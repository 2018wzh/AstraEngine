from hashlib import sha256
import tempfile
from pathlib import Path
import unittest

from director_lingo import DirectorLingoError, build_lingo_ir


class DirectorLingoTests(unittest.TestCase):
    def test_parses_typed_handler_control_flow(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "native-assets" / "test.ls"
            source.parent.mkdir(parents=True)
            payload = (
                "global gflag\n"
                "on choose value\n"
                "  if value = 1 then\n"
                "    go(label(\"next\"))\n"
                "  else\n"
                "    return 0\n"
                "  end if\n"
                "end\n"
            ).encode("cp932")
            source.write_bytes(payload)
            converted = {
                "schema": "tsuinosora.projectorrays_converted_resources.v1",
                "resources": [
                    {
                        "chunk_fourcc": "Lscr",
                        "conversion_method": "projectorrays_lscr_decompiled_script",
                        "native_path": "native-assets/test.ls",
                        "source_alias": "data",
                        "source_relative_path": "TEST/chunks/Lscr-1.bin",
                        "source_sha256": "sha256:" + "1" * 64,
                        "cast_library_id": 1,
                        "cast_member_id": 1,
                        "script_number": 1,
                        "script_source_sha256": f"sha256:{sha256(payload).hexdigest()}",
                        "script_source_kind": "movie_script",
                    }
                ],
            }
            detailed, report = build_lingo_ir(root, converted)
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["handler_count"], 1)
            self.assertEqual(report["statement_counts"]["if_begin"], 1)
            self.assertEqual(detailed["scripts"][0]["handlers"][0]["name"], "choose")

    def test_rejects_unclosed_control_flow(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.ls"
            payload = b"on broken\nif 1 = 1 then\nend\n"
            source.write_bytes(payload)
            converted = {
                "schema": "tsuinosora.projectorrays_converted_resources.v1",
                "resources": [
                    {
                        "chunk_fourcc": "Lscr",
                        "conversion_method": "projectorrays_lscr_decompiled_script",
                        "native_path": "source.ls",
                        "source_relative_path": "TEST/chunks/Lscr-1.bin",
                        "script_source_sha256": f"sha256:{sha256(payload).hexdigest()}",
                    }
                ],
            }
            with self.assertRaisesRegex(DirectorLingoError, "open if block"):
                build_lingo_ir(root, converted)


if __name__ == "__main__":
    unittest.main()
