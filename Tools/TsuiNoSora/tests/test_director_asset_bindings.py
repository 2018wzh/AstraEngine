import unittest

from director_asset_bindings import DirectorAssetBindingError, build_asset_binding_ir


def member(name, slot, resource, child, fourcc):
    return {
        "name": name,
        "cast_member": slot,
        "resource_id": resource,
        "cast_type": 1,
        "children": [{"resource_id": child, "fourcc": fourcc}],
    }


def converted(path, fourcc, parent, digest, *, native_extension=None, width=10, height=10):
    record = {
        "source_alias": "casts",
        "source_relative_path": path,
        "chunk_fourcc": fourcc,
        "role": "fixture",
        "native_path": "native-assets/" + path.rsplit("/", 1)[-1],
        "converted_sha256": "sha256:" + digest * 64,
        "byte_size": 10,
        "cast_resource_id": parent,
    }
    if native_extension is not None:
        record["native_path"] = record["native_path"].rsplit(".", 1)[0] + native_extension
    if fourcc == "BITD":
        record["width"] = width
        record["height"] = height
    return record


def score_fixture():
    stage = [
        {
            "channel": channel,
            "cast_library": 2,
            "cast_member": 1,
            "x": 5,
            "y": 5,
            "width": 10,
            "height": 10,
        }
        for channel in (1, 2, 3, 5, 7, 9, 12)
    ]
    opening = {
        "channel": 35,
        "cast_library": 2,
        "cast_member": 1,
        "x": 5,
        "y": 5,
        "width": 10,
        "height": 10,
    }
    return {
        "frames": [
            {"frame": 1, "main": {"tempo": 0}, "sprites": stage},
            {"frame": 10, "main": {"tempo": 247}, "sprites": [opening]},
            {"frame": 11, "main": {"tempo": 0}, "sprites": []},
        ]
    }


def story_movie():
    return {
        "movie_id": "K",
        "source_alias": "data",
        "cast_libraries": ["K", "GENERAL", "AUDIO"],
        "cast_members": [],
        "labels": [
            {"label": "op", "frame": 10, "action": {"source_sha256": "sha256:" + "1" * 64}},
            {"label": "next", "frame": 12},
        ],
        "score": score_fixture(),
        "score_source_sha256": "sha256:" + "2" * 64,
    }


class DirectorAssetBindingTests(unittest.TestCase):
    def test_resolves_cast_precedence_audio_and_derived_eye_members(self):
        source = {
            "schema": "tsuinosora.director_story_source.v1",
            "external_casts": {
                "GENERAL": [member("background", 1, 10, 11, "BITD"), member("eye1", 2, 12, 13, "BITD")],
                "AUDIO": [member("music", 1, 20, 21, "snd ")],
            },
            "movies": [story_movie()],
        }
        semantics = {
            "schema": "tsuinosora.director_scene_semantic_ir.v1",
            "scenes": [
                {
                    "movie_id": "K",
                    "frame": 1,
                    "source_resource_id": 1,
                    "source_sha256": "sha256:" + "f" * 64,
                    "operations": [
                        {"kind": "show_member", "layer": "background", "member": "background", "opacity": 100},
                        {"kind": "play_audio", "bus": "bgm", "member": "music", "looped": True, "fade_frames": 240},
                        {"kind": "show_eye", "member_suffix": "1"},
                    ],
                }
            ],
        }
        resources = {
            "schema": "tsuinosora.projectorrays_converted_resources.v1",
            "status": "pass",
            "resources": [
                converted("GENERAL/GENERAL/chunks/BITD-11.bin", "BITD", 10, "a"),
                converted("GENERAL/GENERAL/chunks/BITD-13.bin", "BITD", 12, "b"),
                converted("AUDIO/AUDIO/chunks/snd -21.bin", "snd ", 20, "c"),
                converted(
                    "AUDIO/AUDIO/chunks/ediM-22.bin",
                    "ediM",
                    20,
                    "d",
                    native_extension=".mp3",
                ),
            ],
        }

        detailed, report = build_asset_binding_ir(source, semantics, resources)

        self.assertEqual(report["status"], "pass")
        self.assertEqual(report["reference_count"], 6)
        self.assertEqual(report["unique_asset_count"], 3)
        self.assertEqual(report["binding_kind_counts"]["score_opening_media"], 1)
        self.assertEqual(report["binding_kind_counts"]["score_initial_media"], 2)
        stage = detailed["stage_layouts"][0]["layers"]
        self.assertTrue(stage["sky"]["initial_visible"])
        self.assertTrue(stage["character"]["initial_visible"])
        self.assertFalse(stage["background"]["initial_visible"])
        self.assertEqual(stage["character"]["binding"]["director_member"], "background")
        self.assertIn("asset_id", stage["character"]["binding"])
        self.assertTrue(all("asset_id" in item["binding"] for item in detailed["scenes"][0]["operations"]))
        self.assertTrue(
            detailed["scenes"][0]["operations"][1]["binding"]["native_path"].endswith(".mp3")
        )

    def test_missing_member_is_blocking(self):
        source = {
            "schema": "tsuinosora.director_story_source.v1",
            "external_casts": {"GENERAL": [member("eye1", 1, 10, 11, "BITD")]},
            "movies": [{**story_movie(), "cast_libraries": ["K", "GENERAL"]}],
        }
        semantics = {
            "schema": "tsuinosora.director_scene_semantic_ir.v1",
            "scenes": [{"movie_id": "K", "frame": 1, "operations": [{"kind": "show_eye", "member_suffix": "1"}, {"kind": "show_member", "layer": "background", "member": "missing", "opacity": 100}]}],
        }
        resources = {
            "schema": "tsuinosora.projectorrays_converted_resources.v1",
            "status": "pass",
            "resources": [converted("GENERAL/GENERAL/chunks/BITD-11.bin", "BITD", 10, "a")],
        }

        with self.assertRaises(DirectorAssetBindingError):
            build_asset_binding_ir(source, semantics, resources)


if __name__ == "__main__":
    unittest.main()
