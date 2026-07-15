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


def converted(path, fourcc, parent, digest):
    return {
        "source_alias": "casts",
        "source_relative_path": path,
        "chunk_fourcc": fourcc,
        "role": "fixture",
        "native_path": "native-assets/" + path.rsplit("/", 1)[-1],
        "converted_sha256": "sha256:" + digest * 64,
        "byte_size": 10,
        "cast_resource_id": parent,
    }


class DirectorAssetBindingTests(unittest.TestCase):
    def test_resolves_cast_precedence_audio_and_derived_eye_members(self):
        source = {
            "schema": "tsuinosora.director_story_source.v1",
            "external_casts": {
                "GENERAL": [member("background", 1, 10, 11, "BITD"), member("eye1", 2, 12, 13, "BITD")],
                "AUDIO": [member("music", 1, 20, 21, "snd ")],
            },
            "movies": [
                {
                    "movie_id": "K",
                    "source_alias": "data",
                    "cast_libraries": ["K", "GENERAL", "AUDIO"],
                    "cast_members": [],
                }
            ],
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
            ],
        }

        detailed, report = build_asset_binding_ir(source, semantics, resources)

        self.assertEqual(report["status"], "pass")
        self.assertEqual(report["reference_count"], 3)
        self.assertEqual(report["unique_asset_count"], 3)
        self.assertTrue(all("asset_id" in item["binding"] for item in detailed["scenes"][0]["operations"]))

    def test_missing_member_is_blocking(self):
        source = {
            "schema": "tsuinosora.director_story_source.v1",
            "external_casts": {"GENERAL": [member("eye1", 1, 10, 11, "BITD")]},
            "movies": [{"movie_id": "K", "source_alias": "data", "cast_libraries": ["K", "GENERAL"], "cast_members": []}],
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
