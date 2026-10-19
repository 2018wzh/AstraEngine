import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from Tools.AstraEMU import package_client, stage_core


class PackageClientTests(unittest.TestCase):
    def setUp(self):
        self.temporary = TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.manager = self.file("build/astra-emu-manager.exe", b"public Manager artifact")
        self.runtime = self.file("build/runtime.dll", b"public runtime artifact")
        self.output = self.root / "client"
        # Synthetic inputs exercise byte-preserving packaging, not PE or legal validation.
        self.license = self.file("cargo/registry/src/public/luau0-src-0.12.3+luau663/LICENSE",
                                 b"public synthetic license fixture\n")
        self.material = self.file("ui/material-1.1.0/LICENSE.md", b"public material license\n")
        self.symbols = self.file("ui/icons/LICENSE", b"public symbols license\n")
        self.addCleanup(patch.stopall)
        patch.dict(os.environ, {"CARGO_HOME": str(self.root / "cargo")}).start()
        self.core = self.stage_core("astra_emu_example", b"public Family artifact",
                                    {"avcodec-62.dll": b"public dependency"})
        self.metadata = self.root / "metadata-dist/v1"
        self.metadata_files = {
            "compatibility.json": b'{"schema":"astra.emu.compatibility.v3","entries":[]}',
            "presets.json": b'{"schema":"astra.emu.presets.v1","entries":[]}',
            "cores.json": b'{"schema":"astra.emu.cores.v1","entries":[]}',
            "patches/catalog.json": b"[]",
            "patches/common/binary.luau": b"return {}\n",
        }
        self.write_metadata()

    def file(self, relative, content):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return path

    def stage_core(self, stem, content, dependencies=None):
        library = self.file(f"build/{stem}/{stem}.dll", content)
        extra = [self.file(f"build/{stem}/{name}", data) for name, data in (dependencies or {}).items()]
        return stage_core.stage(library, "astra.emu." + stem, "0.2.0", self.root / "staged" / stem,
                                dependencies=extra)

    def write_metadata(self, records=None):
        for relative, content in self.metadata_files.items():
            path = self.metadata / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        if records is None:
            records = [{"path": relative, "sha256": hashlib.sha256(content).hexdigest(), "size": len(content)}
                       for relative, content in sorted(self.metadata_files.items())]
        index = {"schema": "astra.emu.metadata.index.v1", "generated_at_unix_ms": 1,
                 "source_commit": "0" * 40, "files": records}
        (self.metadata / "index.json").write_text(json.dumps(index), encoding="utf-8")

    def package(self, cores=None, runtimes=None, **options):
        options.setdefault("material_license", self.material)
        options.setdefault("symbols_license", self.symbols)
        return package_client.package(
            self.manager, [self.core] if cores is None else cores, self.output,
            [self.runtime] if runtimes is None else runtimes, **options)

    def assert_rejected(self, **options):
        with self.assertRaises((ValueError, OSError)):
            self.package(**options)
        self.assertFalse(self.output.exists())
        self.assertEqual([], list(self.root.glob(".astraemu-client-*")))

    def test_core_directories_and_licenses_are_preserved(self):
        expected = {
            self.manager: (self.manager.name, b"public Manager artifact"),
            self.runtime: (self.runtime.name, b"public runtime artifact"),
            self.license: ("licenses/luau/LICENSE", b"public synthetic license fixture\n"),
            self.material: ("licenses/slint-material/LICENSE.md", b"public material license\n"),
            self.symbols: ("licenses/material-symbols/LICENSE", b"public symbols license\n"),
        }
        self.package()
        for source, (relative, content) in expected.items():
            self.assertEqual(content, source.read_bytes())
            self.assertEqual(content, (self.output / relative).read_bytes())
        shipped = {p.relative_to(self.output / "cores").as_posix()
                   for p in (self.output / "cores").rglob("*") if p.is_file()}
        self.assertEqual({"astra_emu_example/astra_emu_example.dll", "astra_emu_example/avcodec-62.dll",
                          "astra_emu_example/core.json"}, shipped)
        for name in ("astra_emu_example.dll", "avcodec-62.dll", "core.json"):
            self.assertEqual((self.core / name).read_bytes(),
                             (self.output / "cores/astra_emu_example" / name).read_bytes())
        repo = Path(package_client.__file__).resolve().parents[2]
        for relative, source in (("README.md", repo / "Docs/manual/astraemu-client.md"),
                                 ("THIRD_PARTY_NOTICES.md", repo / "Emulator/THIRD_PARTY_NOTICES.md"),
                                 ("licenses/mlua/LICENSE", repo / "Emulator/ThirdParty/mlua/LICENSE")):
            self.assertEqual(source.read_bytes(), (self.output / relative).read_bytes())
        self.assertFalse((self.output / "metadata").exists())
        self.assertFalse((self.output / "patches").exists())
        self.assertEqual({self.manager.name}, {p.name for p in self.output.glob("*.exe")})

    def test_multiple_cores_each_keep_their_directory(self):
        second = self.stage_core("astra_emu_second", b"second Family artifact")
        self.package(cores=[self.core, second])
        self.assertEqual({"astra_emu_example", "astra_emu_second"},
                         {p.name for p in (self.output / "cores").iterdir()})
        self.assertEqual(b"second Family artifact",
                         (self.output / "cores/astra_emu_second/astra_emu_second.dll").read_bytes())

    def test_metadata_dist_copies_indexed_files_only(self):
        (self.metadata / "unlisted.json").write_bytes(b"must not ship")
        self.package(metadata_dist=self.metadata)
        shipped = {p.relative_to(self.output / "metadata").as_posix()
                   for p in (self.output / "metadata").rglob("*") if p.is_file()}
        self.assertEqual(set(self.metadata_files) | {"index.json"}, shipped)
        for relative, content in self.metadata_files.items():
            self.assertEqual(content, (self.output / "metadata" / relative).read_bytes())
        self.assertEqual((self.metadata / "index.json").read_bytes(),
                         (self.output / "metadata/index.json").read_bytes())

    def test_metadata_without_index_or_with_bad_records_is_rejected(self):
        (self.metadata / "index.json").unlink()
        self.assert_rejected(metadata_dist=self.metadata)
        good = [{"path": relative, "sha256": hashlib.sha256(content).hexdigest(), "size": len(content)}
                for relative, content in sorted(self.metadata_files.items())]
        for records in (
            [{**good[0], "sha256": "0" * 64}],
            [{**good[0], "size": good[0]["size"] + 1}],
            [{**good[0], "path": "../outside.json"}],
            [{**good[0], "path": "Upper.json"}],
            [good[0], good[0]],
            [{"path": "missing.json", "sha256": "0" * 64, "size": 0}],
            [{**good[0], "size": 9 * 1024 * 1024}],
        ):
            with self.subTest(records=records):
                self.write_metadata(records)
                self.assert_rejected(metadata_dist=self.metadata)
        (self.metadata / "index.json").write_text('{"schema":"other","files":[]}', encoding="utf-8")
        self.assert_rejected(metadata_dist=self.metadata)

    def test_subtitle_license_closure_copies_exact_public_inputs(self):
        share = self.root / "subtitle/share"
        ports = ("libass", "freetype", "fribidi", "harfbuzz", "brotli", "bzip2", "libpng", "zlib")
        for port in ports:
            self.file(f"subtitle/share/{port}/copyright", ("public " + port + " notice\n").encode())
        self.file("subtitle/share/fribidi/vcpkg_abi_info.txt", b"build metadata should not ship")
        self.package(subtitle_share=share)
        for port in ports:
            self.assertEqual(("public " + port + " notice\n").encode(),
                             (self.output / "licenses/subtitle" / port / "copyright").read_bytes())
        self.assertEqual(set(ports),
                         {p.parent.name for p in (self.output / "licenses/subtitle").rglob("copyright")})
        self.assertFalse((self.output / "licenses/subtitle/fribidi/vcpkg_abi_info.txt").exists())

    def test_missing_subtitle_license_fails_without_output(self):
        for port in ("libass", "freetype", "fribidi", "harfbuzz", "brotli", "bzip2", "libpng"):
            self.file(f"subtitle/share/{port}/copyright", b"public notice")
        self.assert_rejected(subtitle_share=self.root / "subtitle/share")

    def test_missing_material_licenses_fail_without_output(self):
        self.assert_rejected(material_license=self.root / "missing/LICENSE.md")
        self.assert_rejected(symbols_license=self.root / "missing/LICENSE")

    def test_missing_luau_source_requires_explicit_license(self):
        with patch.dict(os.environ, {"CARGO_HOME": str(self.root / "empty-cargo")}):
            self.assert_rejected()
            self.package(luau_license=self.license)
        self.assertEqual(self.license.read_bytes(), (self.output / "licenses/luau/LICENSE").read_bytes())

    def test_core_directory_must_match_its_manifest(self):
        cases = []
        tampered = self.stage_core("astra_emu_tampered", b"original")
        (tampered / "astra_emu_tampered.dll").write_bytes(b"tampered")
        cases.append(tampered)
        extra = self.stage_core("astra_emu_extra", b"core")
        (extra / "unlisted.dll").write_bytes(b"unlisted")
        cases.append(extra)
        renamed = self.stage_core("astra_emu_renamed", b"core")
        cases.append(renamed.rename(renamed.parent / "astra_emu_other"))
        bare = self.root / "bare/astra_emu_bare"
        bare.mkdir(parents=True)
        (bare / "astra_emu_bare.dll").write_bytes(b"no manifest")
        cases.append(bare)
        second = self.stage_core("astra_emu_two", b"core")
        manifest = json.loads((second / "core.json").read_text(encoding="utf-8"))
        (second / "astra_emu_three.dll").write_bytes(b"second library")
        manifest["files"].append({"path": "astra_emu_three.dll",
                                  "sha256": hashlib.sha256(b"second library").hexdigest()})
        (second / "core.json").write_text(json.dumps(manifest), encoding="utf-8")
        cases.append(second)
        for directory in cases:
            with self.subTest(directory=directory.name):
                self.assert_rejected(cores=[directory])
        self.assert_rejected(cores=[])
        self.assert_rejected(cores=[self.core, self.core])

    def test_copy_failure_cleans_staging_and_preserves_sources(self):
        with patch.object(package_client.shutil, "copy2", side_effect=OSError("public injected failure")):
            self.assert_rejected(metadata_dist=self.metadata)
        self.assertEqual(b"public Manager artifact", self.manager.read_bytes())
        self.assertEqual(b"public Family artifact", (self.core / "astra_emu_example.dll").read_bytes())

    def test_dangling_output_link_is_not_followed(self):
        missing = self.root / "missing-target"
        try:
            self.output.symlink_to(missing, target_is_directory=True)
        except OSError:
            self.skipTest("symlink creation requires platform permission")
        with self.assertRaises(ValueError):
            self.package()
        self.assertTrue(self.output.is_symlink())
        self.assertFalse(missing.exists())

    def test_existing_output_and_its_contents_are_untouched(self):
        sentinel = self.file("client/user.dat", b"user data")
        with self.assertRaises(ValueError):
            self.package(metadata_dist=self.metadata)
        self.assertEqual(b"user data", sentinel.read_bytes())
        self.assertEqual([sentinel], list(self.output.iterdir()))

    def test_output_created_during_copy_is_not_replaced_even_when_empty(self):
        copy = shutil.copy2
        def create_output(source, destination, *args, **kwargs):
            self.output.mkdir(exist_ok=True)
            return copy(source, destination, *args, **kwargs)
        with patch.object(package_client.shutil, "copy2", side_effect=create_output):
            with self.assertRaises(OSError):
                self.package()
        self.assertTrue(self.output.is_dir())
        self.assertEqual([], list(self.output.iterdir()))
        self.assertEqual([], list(self.root.glob(".astraemu-client-*")))

    def test_output_inside_an_input_directory_is_rejected(self):
        self.output = self.metadata / "client"
        self.assert_rejected(metadata_dist=self.metadata)
        self.output = self.core / "client"
        self.assert_rejected()

    def test_case_insensitive_runtime_and_reserved_name_collisions(self):
        for filename in ("ASTRA-EMU-MANAGER.EXE", "README.md", "licenses", "metadata", "cores",
                         "astra-emu-patch", "ASTRA-EMU-PATCH.EXE"):
            with self.subTest(filename=filename):
                duplicate = self.file("other/" + filename, b"collision")
                with self.assertRaises(ValueError):
                    self.package(runtimes=[duplicate])
                self.assertFalse(self.output.exists())
                self.assertEqual(b"collision", duplicate.read_bytes())
        duplicate = self.file("other/RUNTIME.DLL", b"other runtime")
        with self.assertRaises(ValueError):
            self.package(runtimes=[self.runtime, duplicate])
        self.assertFalse(self.output.exists())

    def test_manager_cannot_overwrite_reserved_document(self):
        self.manager = self.file("other/README.md", b"Manager")
        self.assert_rejected()

    def test_missing_manager_and_runtime_fail_without_output(self):
        self.manager = self.root / "missing"
        self.assert_rejected()
        self.manager = self.file("build/astra-emu-manager.exe", b"public Manager artifact")
        self.assert_rejected(runtimes=[self.root / "missing"])

    def test_cli_invocation_produces_real_files_and_refuses_existing_output(self):
        command = [sys.executable, str(Path(package_client.__file__).resolve()),
                   "--manager", str(self.manager), "--core-dir", str(self.core),
                   "--runtime-file", str(self.runtime), "--output", str(self.output),
                   "--material-license", str(self.material), "--symbols-license", str(self.symbols)]
        result = subprocess.run(command, capture_output=True)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(self.runtime.read_bytes(), (self.output / self.runtime.name).read_bytes())
        shutil.rmtree(self.output)
        result = subprocess.run(command + ["--metadata-dist", str(self.metadata),
                                          "--luau-license", str(self.license)], capture_output=True)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(self.metadata_files["patches/common/binary.luau"],
                         (self.output / "metadata/patches/common/binary.luau").read_bytes())
        self.assertEqual({"astra_emu_example"}, {p.name for p in (self.output / "cores").iterdir()})
        before = (self.output / self.manager.name).read_bytes()
        result = subprocess.run(command, capture_output=True)
        self.assertEqual(1, result.returncode)
        self.assertEqual(before, (self.output / self.manager.name).read_bytes())

    def test_cli_rejects_retired_arguments(self):
        for retired in (["--core", str(self.core / "astra_emu_example.dll")],
                        ["--patches", str(self.metadata)], ["--patch-common", "common/binary.luau"]):
            with self.subTest(retired=retired[0]):
                command = [sys.executable, str(Path(package_client.__file__).resolve()),
                           "--manager", str(self.manager), "--core-dir", str(self.core),
                           "--output", str(self.output), *retired]
                result = subprocess.run(command, capture_output=True)
                self.assertNotEqual(0, result.returncode)
                self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
