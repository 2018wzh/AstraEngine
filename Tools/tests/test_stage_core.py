import hashlib
import json
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from Tools.AstraEMU import stage_core


class StageCoreTests(unittest.TestCase):
    def setUp(self):
        self.temporary = TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.library = self.file("build/astra_emu_example.dll", b"public Family artifact")
        self.dependency = self.file("build/avcodec-62.dll", b"public dependency")
        self.license = self.file("licenses/LICENSE-MPL-2.0", b"public license text\n")
        self.output = self.root / "staged"

    def file(self, relative, content):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return path

    def stage(self, **options):
        return stage_core.stage(self.library, "astra.emu.example", "0.2.0", self.output, **options)

    def assert_rejected(self, **options):
        with self.assertRaises((ValueError, OSError)):
            self.stage(**options)
        self.assertFalse(self.output.exists())
        self.assertEqual([], list(self.root.glob(".astraemu-core-*")))

    def test_layout_and_manifest_hash_every_file(self):
        resources = self.root / "Resources"
        self.file("Resources/ui/theme.csb", b"resource bytes")
        staged = self.stage(dependencies=[self.dependency], resources=resources,
                            licenses=[str(self.license), "ffmpeg/COPYING.LGPLv2.1=" + str(self.license)])
        self.assertEqual(self.output / "astra_emu_example", staged)
        manifest = json.loads((staged / "core.json").read_text(encoding="utf-8"))
        self.assertEqual("astra.emu.core_package.v1", manifest["schema"])
        self.assertEqual("astra.emu.example", manifest["plugin_id"])
        self.assertEqual("astra_emu_example.dll", manifest["library"])
        self.assertEqual("0.2.0", manifest["version"])
        self.assertEqual("astra.emu.native.c.v3", manifest["abi"])
        expected = {"astra_emu_example.dll", "avcodec-62.dll", "Resources/ui/theme.csb",
                    "licenses/LICENSE-MPL-2.0", "licenses/ffmpeg/COPYING.LGPLv2.1"}
        self.assertEqual(expected, {record["path"] for record in manifest["files"]})
        for record in manifest["files"]:
            content = (staged / record["path"]).read_bytes()
            self.assertEqual(hashlib.sha256(content).hexdigest(), record["sha256"])
        on_disk = {p.relative_to(staged).as_posix() for p in staged.rglob("*") if p.is_file()}
        self.assertEqual(expected | {"core.json"}, on_disk)
        self.assertEqual(sorted(record["path"] for record in manifest["files"]),
                         [record["path"] for record in manifest["files"]])

    def test_output_must_be_new_and_is_untouched(self):
        sentinel = self.file("staged/user.dat", b"user data")
        with self.assertRaises(ValueError):
            self.stage()
        self.assertEqual([sentinel], list(self.output.iterdir()))

    def test_second_family_library_is_rejected(self):
        other = self.file("build/astra_emu_other.dll", b"other core")
        self.assert_rejected(dependencies=[other])

    def test_non_family_library_is_rejected(self):
        self.library = self.dependency
        self.assert_rejected()

    def test_duplicate_and_reserved_names_are_rejected(self):
        duplicate = self.file("other/AVCODEC-62.DLL", b"duplicate")
        self.assert_rejected(dependencies=[self.dependency, duplicate])
        reserved = self.file("other/core.json", b"{}")
        self.assert_rejected(dependencies=[reserved])
        self.assert_rejected(licenses=[str(self.license), str(self.license)])

    def test_invalid_identity_and_missing_inputs_are_rejected(self):
        for plugin_id, version in (("", "0.2.0"), ("../x", "0.2.0"), ("astra.emu.x", ""), ("astra.emu.x", "1 0")):
            with self.subTest(plugin_id=plugin_id, version=version):
                with self.assertRaises(ValueError):
                    stage_core.stage(self.library, plugin_id, version, self.output)
        self.assert_rejected(dependencies=[self.root / "missing.dll"])
        self.assert_rejected(resources=self.root / "missing")
        (self.root / "empty").mkdir()
        self.assert_rejected(resources=self.root / "empty")

    def test_copy_failure_cleans_staging(self):
        with patch.object(stage_core.shutil, "copy2", side_effect=OSError("public injected failure")):
            self.assert_rejected(dependencies=[self.dependency])
        self.assertEqual(b"public Family artifact", self.library.read_bytes())

    def test_cli_produces_directory_and_fails_on_existing_output(self):
        command = [sys.executable, str(Path(stage_core.__file__).resolve()),
                   "--library", str(self.library), "--plugin-id", "astra.emu.example",
                   "--version", "0.2.0", "--dependency", str(self.dependency),
                   "--license", str(self.license), "--output", str(self.output)]
        result = subprocess.run(command, capture_output=True)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertTrue((self.output / "astra_emu_example/core.json").is_file())
        result = subprocess.run(command, capture_output=True)
        self.assertEqual(1, result.returncode)


if __name__ == "__main__":
    unittest.main()
