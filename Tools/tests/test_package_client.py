import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from Tools.AstraEMU import package_client


class PackageClientTests(unittest.TestCase):
    def setUp(self):
        self.temporary = TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.manager = self.file("build/astra-emu-manager.exe", b"public Manager artifact")
        self.core = self.file("build/astra_emu_example.dll", b"public Family artifact")
        self.runtime = self.file("build/runtime.dll", b"public runtime artifact")
        self.output = self.root / "client"
        # Synthetic inputs exercise byte-preserving packaging, not PE or legal validation.
        self.license = self.file("cargo/registry/src/public/luau0-src-0.12.3+luau663/LICENSE",
                                 b"public synthetic license fixture\n")
        self.addCleanup(patch.stopall)
        patch.dict(os.environ, {"CARGO_HOME": str(self.root / "cargo")}).start()
        self.patches = self.root / "public-patches"
        self.catalog([{"id": "legacy-normalizer", "title": "Public legacy resource normalizer",
                       "script": "legacy-normalizer.luau"}])
        # Deliberately invalid script: packaging and catalog handling never execute it.
        self.entry = self.file("public-patches/legacy-normalizer.luau", b"not executable Luau !\n")
        self.module = self.file("public-patches/common/binary.luau", b"return {}\n")

    def file(self, relative, content):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return path

    def catalog(self, records):
        return self.file("public-patches/catalog.json", json.dumps(records).encode("utf-8"))

    def package(self, **options):
        return package_client.package(self.manager, [self.core], self.output, [self.runtime], **options)

    def assert_rejected(self, **options):
        with self.assertRaises((ValueError, OSError)):
            self.package(**options)
        self.assertFalse(self.output.exists())
        self.assertEqual([], list(self.root.glob(".astraemu-client-*")))

    def test_original_four_argument_call_preserves_files_and_layout(self):
        expected = {
            self.manager: (self.manager.name, b"public Manager artifact"),
            self.runtime: (self.runtime.name, b"public runtime artifact"),
            self.core: ("cores/" + self.core.name, b"public Family artifact"),
            self.license: ("licenses/luau/LICENSE", b"public synthetic license fixture\n"),
        }
        self.package()
        for source, (relative, content) in expected.items():
            self.assertEqual(content, source.read_bytes())
            self.assertEqual(content, (self.output / relative).read_bytes())
        repo = Path(package_client.__file__).resolve().parents[2]
        for relative, source in (("README.md", repo / "Docs/manual/astraemu-client.md"),
                                 ("THIRD_PARTY_NOTICES.md", repo / "Emulator/THIRD_PARTY_NOTICES.md"),
                                 ("licenses/mlua/LICENSE", repo / "Emulator/ThirdParty/mlua/LICENSE")):
            self.assertEqual(source.read_bytes(), (self.output / relative).read_bytes())
        self.assertFalse((self.output / "patches").exists())
        self.assertEqual({self.manager.name}, {p.name for p in self.output.glob("*.exe")})

    def test_package_contains_one_manager_selected_cores_and_only_trusted_assets(self):
        for name in ("fixtures/secret.dat", "test.luau", "private/key.toml", "game.exe",
                     "unlisted.luau", "common/unused.luau", "common/fixture.luau"):
            self.file("public-patches/" + name, b"must not ship")
        selected = self.file("build/astra_emu_second.dll", b"second selected Family artifact")
        self.file("build/astra_emu_unselected.dll", b"must not ship")
        package_client.package(
            self.manager, [self.core, selected], self.output, [self.runtime], self.patches,
            patch_common=["common/binary.luau"],
        )
        self.assertEqual({self.manager.name}, {p.name for p in self.output.glob("*.exe")})
        self.assertEqual({self.core.name, selected.name},
                         {p.name for p in (self.output / "cores").iterdir()})
        for core in (self.core, selected):
            self.assertEqual(core.read_bytes(), (self.output / "cores" / core.name).read_bytes())
        shipped = {p.relative_to(self.output / "patches").as_posix()
                   for p in (self.output / "patches").rglob("*") if p.is_file()}
        self.assertEqual({"catalog.json", "legacy-normalizer.luau", "common/binary.luau"}, shipped)
        self.assertEqual(self.entry.read_bytes(), (self.output / "patches" / self.entry.name).read_bytes())
        self.assertEqual(self.module.read_bytes(), (self.output / "patches/common/binary.luau").read_bytes())
        self.assertEqual((self.patches / "catalog.json").read_bytes(),
                         (self.output / "patches/catalog.json").read_bytes())

    def test_empty_catalog_does_not_invent_recipes(self):
        self.catalog([])
        self.package(patches=self.patches)
        self.assertEqual(b"[]", (self.output / "patches/catalog.json").read_bytes())
        self.assertEqual(["catalog.json"], [p.name for p in (self.output / "patches").iterdir()])

    def test_subtitle_static_license_closure_copies_exact_public_inputs(self):
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

    def test_missing_luau_source_requires_explicit_license(self):
        with patch.dict(os.environ, {"CARGO_HOME": str(self.root / "empty-cargo")}):
            self.assert_rejected()
            self.package(luau_license=self.license)
        self.assertEqual(self.license.read_bytes(), (self.output / "licenses/luau/LICENSE").read_bytes())

    def test_copy_failure_cleans_staging_and_preserves_sources(self):
        with patch.object(package_client.shutil, "copy2", side_effect=OSError("public injected failure")):
            self.assert_rejected(patches=self.patches)
        self.assertEqual(b"public Manager artifact", self.manager.read_bytes())
        self.assertEqual(b"public Family artifact", self.core.read_bytes())

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
            self.package(patches=self.patches)
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

    def test_case_insensitive_runtime_and_source_basename_collisions(self):
        for filename in ("ASTRA-EMU-MANAGER.EXE", "ASTRA_EMU_EXAMPLE.DLL", "README.md",
                         "licenses", "patches"):
            with self.subTest(filename=filename):
                duplicate = self.file("other/" + filename, b"collision")
                with self.assertRaises(ValueError):
                    package_client.package(self.manager, [self.core], self.output, [duplicate])
                self.assertFalse(self.output.exists())

    def test_retired_patch_executable_names_are_reserved_for_runtime_files(self):
        for filename in ("astra-emu-patch", "astra-emu-patch.exe",
                         "ASTRA-EMU-PATCH", "ASTRA-EMU-PATCH.EXE"):
            with self.subTest(filename=filename):
                retired = self.file("other/" + filename, b"retired executable")
                with self.assertRaises(ValueError):
                    package_client.package(self.manager, [self.core], self.output, [retired])
                self.assertFalse(self.output.exists())
                self.assertEqual(b"retired executable", retired.read_bytes())
                self.assertEqual([], list(self.root.glob(".astraemu-client-*")))

    def test_manager_cannot_overwrite_reserved_document(self):
        self.manager = self.file("other/README.md", b"Manager")
        self.assert_rejected()

    def test_duplicate_core_and_runtime_basenames_are_rejected(self):
        duplicate = self.file("other/astra_emu_example.dll", b"other core")
        with self.assertRaises(ValueError):
            package_client.package(self.manager, [self.core, duplicate], self.output, [])
        duplicate = self.file("other/RUNTIME.DLL", b"other runtime")
        with self.assertRaises(ValueError):
            package_client.package(self.manager, [self.core], self.output, [self.runtime, duplicate])
        self.assertFalse(self.output.exists())

    def test_catalog_escapes_devices_non_scripts_and_scaffolds_are_rejected(self):
        for name in ("../outside.luau", "/absolute.luau", "C:/outside.luau", "sub/entry.luau",
                     "..\\outside.luau", "entry.luau:stream", "NUL.luau", "COM1.luau",
                     "fixture.luau", "entry.exe", "entry.luau."):
            with self.subTest(name=name):
                self.catalog([{"id": "public", "title": "Public", "script": name}])
                if name == "fixture.luau":
                    self.file("public-patches/fixture.luau", b"return {}")
                self.assert_rejected(patches=self.patches)

    def test_duplicate_catalog_ids_and_casefold_paths_are_rejected(self):
        for records in (
            [{"id": "same", "title": "One", "script": "legacy-normalizer.luau"}] * 2,
            [{"id": "one", "title": "One", "script": "legacy-normalizer.luau"},
             {"id": "two", "title": "Two", "script": "LEGACY-normalizer.luau"}],
        ):
            self.catalog(records)
            self.file("public-patches/LEGACY-normalizer.luau", b"invalid Luau")
            self.assert_rejected(patches=self.patches)

    def test_common_paths_are_explicit_and_cannot_escape(self):
        for common in (["../outside.luau"], ["common/../outside.luau"], ["common/key.toml"],
                       ["common/test.luau"], ["common/binary.luau", "common/binary.luau"]):
            with self.subTest(common=common):
                self.file("public-patches/common/test.luau", b"return {}")
                self.assert_rejected(patches=self.patches, patch_common=common)
        self.assert_rejected(patch_common=["common/binary.luau"])

    def test_missing_catalog_entry_or_module_is_rejected(self):
        self.assert_rejected(patches=self.root / "missing-patches")
        self.assert_rejected(patches=self.patches, patch_common=["common/missing.luau"])
        self.entry.unlink()
        self.assert_rejected(patches=self.patches)

    def test_malformed_catalog_is_rejected(self):
        for records in ({"recipes": []}, [None], [{"id": "public", "title": "Public"}],
                        [{"id": "public", "title": "Public", "script": 42}],
                        [{"id": "", "title": "Public", "script": "legacy-normalizer.luau"}]):
            with self.subTest(records=records):
                self.catalog(records)
                self.assert_rejected(patches=self.patches)
        self.file("public-patches/catalog.json", b"{not json}")
        self.assert_rejected(patches=self.patches)

    def test_missing_manager_runtime_and_invalid_core_fail_without_output(self):
        for manager, cores, runtimes in (
            (self.root / "missing", [self.core], []),
            (self.manager, [], []),
            (self.manager, [self.runtime], []),
            (self.manager, [self.core], [self.root / "missing"]),
            (self.manager, [self.root / "build"], []),
        ):
            with self.subTest(manager=manager, cores=cores, runtimes=runtimes):
                with self.assertRaises((ValueError, OSError)):
                    package_client.package(manager, cores, self.output, runtimes)
                self.assertFalse(self.output.exists())

    def test_patch_output_overlap_is_rejected(self):
        self.output = self.patches / "client"
        self.assert_rejected(patches=self.patches)

    def test_linked_patch_asset_is_rejected_and_target_unchanged(self):
        outside = self.file("outside.luau", b"outside input")
        self.entry.unlink()
        try:
            self.entry.symlink_to(outside)
        except OSError:
            self.skipTest("symlink creation requires platform permission")
        self.assert_rejected(patches=self.patches)
        self.assertEqual(b"outside input", outside.read_bytes())

    def test_cli_optional_arguments_and_original_invocation_produce_real_files(self):
        command = [sys.executable, str(Path(package_client.__file__).resolve()),
                   "--manager", str(self.manager), "--core", str(self.core),
                   "--runtime-file", str(self.runtime), "--output", str(self.output)]
        result = subprocess.run(command, capture_output=True)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(self.runtime.read_bytes(), (self.output / self.runtime.name).read_bytes())
        shutil.rmtree(self.output)
        result = subprocess.run(command + ["--patches", str(self.patches),
                                          "--patch-common", "common/binary.luau",
                                          "--luau-license", str(self.license)], capture_output=True)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(self.entry.read_bytes(), (self.output / "patches" / self.entry.name).read_bytes())
        self.assertEqual({self.manager.name}, {p.name for p in self.output.glob("*.exe")})
        self.assertEqual({self.core.name}, {p.name for p in (self.output / "cores").iterdir()})
        self.assertEqual({"catalog.json", "legacy-normalizer.luau", "common/binary.luau"},
                         {p.relative_to(self.output / "patches").as_posix()
                          for p in (self.output / "patches").rglob("*") if p.is_file()})
        before = (self.output / self.manager.name).read_bytes()
        result = subprocess.run(command, capture_output=True)
        self.assertEqual(1, result.returncode)
        self.assertEqual(before, (self.output / self.manager.name).read_bytes())

    def test_cli_rejects_retired_patch_executable_as_a_runtime_file(self):
        retired = self.file("build/ASTRA-EMU-PATCH.EXE", b"retired executable")
        command = [sys.executable, str(Path(package_client.__file__).resolve()),
                   "--manager", str(self.manager), "--core", str(self.core),
                   "--runtime-file", str(retired), "--output", str(self.output),
                   "--luau-license", str(self.license)]
        result = subprocess.run(command, capture_output=True)
        self.assertEqual(1, result.returncode)
        self.assertFalse(self.output.exists())
        self.assertEqual([], list(self.root.glob(".astraemu-client-*")))
        self.assertEqual(b"retired executable", retired.read_bytes())


if __name__ == "__main__":
    unittest.main()
