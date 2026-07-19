import tempfile
import unittest
from pathlib import Path
from unittest import mock

import package_release


class PackageReleaseTests(unittest.TestCase):
    def test_hash_mismatch_blocks_before_output_creation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            patcher, helper, license_file = root / "patcher.exe", root / "helper.exe", root / "LICENSE"
            patcher.write_bytes(b"patcher")
            helper.write_bytes(b"wrong")
            license_file.write_bytes(b"wrong")
            output = root / "bundle"
            with self.assertRaisesRegex(ValueError, "helper SHA-256 mismatch"):
                package_release.package(patcher, helper, license_file, root, output)
            self.assertFalse(output.exists())

    def test_bundle_records_only_relative_paths_and_hashes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            patcher, helper, license_file = root / "patcher.exe", root / "helper.exe", root / "LICENSE"
            patcher.write_bytes(b"patcher")
            helper.write_bytes(b"helper")
            license_file.write_bytes(b"license")
            output = root / "bundle"
            locale = root / "locale"
            locale.mkdir()
            locale_hashes = {}
            for name in package_release.LOCALE_FILES:
                (locale / name).write_bytes(name.encode("ascii"))
                locale_hashes[name] = package_release.digest(locale / name)
            with mock.patch.object(package_release, "HELPER_SHA256", package_release.digest(helper)), mock.patch.object(
                package_release, "LICENSE_SHA256", package_release.digest(license_file)
            ), mock.patch.object(package_release, "LOCALE_FILES", locale_hashes):
                manifest = package_release.package(patcher, helper, license_file, locale, output)
            self.assertTrue((output / package_release.PATCHER_NAME).is_file())
            self.assertTrue(all(not Path(item["relative_path"]).is_absolute() for item in manifest["files"]))


if __name__ == "__main__":
    unittest.main()
