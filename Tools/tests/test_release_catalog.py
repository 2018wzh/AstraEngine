import hashlib
import json
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
import zipfile

from Tools.AstraEMU import release_catalog, stage_core


class ReleaseCatalogTests(unittest.TestCase):
    def setUp(self):
        self.temporary = TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        library = self.file("build/astra_emu_example.dll", b"public Family artifact")
        resource = self.file("build/Resources/ui.csb", b"resource")
        self.core = stage_core.stage(library, "astra.emu.example", "0.3.1", self.root / "staged",
                                     resources=resource.parent)
        self.table = {"astra.emu.example": {"display_name": "示例", "family_name": "Example",
                                            "license": "MPL-2.0"}}
        self.base = "https://github.com/2018wzh/AstraEngine/releases/download/emu-v0.2.0/"

    def file(self, relative, content):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return path

    def test_archive_holds_core_directory_contents_at_root_deterministically(self):
        first = release_catalog.write_archive(self.core, "0.2.0", self.root / "a")
        self.assertEqual("astra-emu-core-astra_emu_example-0.2.0-windows-x86_64.zip", first.name)
        with zipfile.ZipFile(first) as archive:
            self.assertEqual(["Resources/ui.csb", "astra_emu_example.dll", "core.json"], archive.namelist())
            self.assertEqual((self.core / "core.json").read_bytes(), archive.read("core.json"))
        second = release_catalog.write_archive(self.core, "0.2.0", self.root / "b")
        self.assertEqual(first.read_bytes(), second.read_bytes())
        with self.assertRaises(ValueError):
            release_catalog.write_archive(self.core, "0.2.0", self.root / "a")

    def test_catalog_entry_describes_the_published_archive(self):
        archive = release_catalog.write_archive(self.core, "0.2.0", self.root / "release")
        [entry] = release_catalog.catalog_entries([archive], self.table, self.base, "emu-v0.2.0")
        self.assertEqual({
            "plugin_id": "astra.emu.example", "display_name": "示例", "family_name": "Example",
            "version": "0.3.1", "abi": "astra.emu.native.c.v3", "platform": "windows-x86_64",
            "url": self.base + archive.name, "sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
            "size": archive.stat().st_size, "license": "MPL-2.0", "release_tag": "emu-v0.2.0",
        }, entry)
        self.assertIn("示例（Example，`astra.emu.example`）0.3.1",
                      release_catalog.release_notes([entry], "emu-v0.2.0"))

    def test_catalog_rejects_unknown_duplicate_and_insecure_inputs(self):
        archive = release_catalog.write_archive(self.core, "0.2.0", self.root / "release")
        with self.assertRaises(ValueError):
            release_catalog.catalog_entries([archive], {}, self.base, "emu-v0.2.0")
        with self.assertRaises(ValueError):
            release_catalog.catalog_entries([archive, archive], self.table, self.base, "emu-v0.2.0")
        with self.assertRaises(ValueError):
            release_catalog.catalog_entries([archive], self.table, "http://example.com/", "emu-v0.2.0")

    def test_crate_version_resolves_explicit_and_workspace_versions(self):
        self.file("ws/Cargo.toml", b'[workspace]\nmembers = ["a", "b"]\n[workspace.package]\nversion = "0.4.0"\n')
        explicit = self.file("ws/a/Cargo.toml", b'[package]\nname = "a"\nversion = "1.2.3"\n')
        inherited = self.file("ws/b/Cargo.toml", b'[package]\nname = "b"\nversion.workspace = true\n')
        self.assertEqual("1.2.3", release_catalog.crate_version(explicit))
        self.assertEqual("0.4.0", release_catalog.crate_version(inherited))
        orphan = self.file("lone/Cargo.toml", b'[package]\nname = "c"\nversion.workspace = true\n')
        with self.assertRaises(ValueError):
            release_catalog.crate_version(orphan)

    def test_cli_writes_entry_files_and_notes(self):
        release = self.root / "release"
        command = [sys.executable, str(Path(release_catalog.__file__).resolve())]
        result = subprocess.run(command + ["archive", "--core-dir", str(self.core), "--version", "0.2.0",
                                           "--output-dir", str(release)], capture_output=True)
        self.assertEqual(0, result.returncode, result.stderr)
        table = self.file("table.json", json.dumps(self.table).encode("utf-8"))
        result = subprocess.run(command + ["catalog", "--archive", str(next(release.glob("*.zip"))),
                                           "--table", str(table), "--base-url", self.base,
                                           "--tag", "emu-v0.2.0", "--output-dir", str(self.root / "entries"),
                                           "--notes", str(self.root / "notes.md")], capture_output=True)
        self.assertEqual(0, result.returncode, result.stderr)
        entry = json.loads((self.root / "entries/astra.emu.example.json").read_text(encoding="utf-8"))
        self.assertEqual("emu-v0.2.0", entry["release_tag"])
        self.assertTrue((self.root / "notes.md").read_text(encoding="utf-8").startswith("AstraEMU emu-v0.2.0"))


if __name__ == "__main__":
    unittest.main()
