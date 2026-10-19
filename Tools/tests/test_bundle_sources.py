from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
import zipfile

from Tools.AstraEMU import bundle_sources


class BundleSourcesTests(unittest.TestCase):
    def setUp(self):
        self.temporary = TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.downloads = self.root / "downloads"
        self.file("downloads/ffmpeg-ffmpeg-n9.0.2.tar.gz", b"upstream archive")
        self.file("downloads/fribidi-fribidi-v1.0.17.tar.gz", b"other archive")
        self.file("downloads/tools/nasm.zip", b"tool")
        self.port = self.root / "ports/ffmpeg"
        self.file("ports/ffmpeg/portfile.cmake", b"vcpkg_from_github()")
        self.file("ports/ffmpeg/vcpkg.json", b"{}")
        self.file("ports/ffmpeg/patches/0001-fix.patch", b"diff")
        self.output = self.root / "out/ffmpeg-source.zip"

    def file(self, relative, content):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return path

    def test_bundle_holds_exact_archive_and_port(self):
        bundle_sources.bundle(self.downloads, ["ffmpeg-ffmpeg-n9.0.*.tar.gz"], [self.port], self.output)
        with zipfile.ZipFile(self.output) as archive:
            self.assertEqual(["downloads/ffmpeg-ffmpeg-n9.0.2.tar.gz", "ports/ffmpeg/patches/0001-fix.patch",
                              "ports/ffmpeg/portfile.cmake", "ports/ffmpeg/vcpkg.json"], archive.namelist())
            self.assertEqual(b"upstream archive", archive.read("downloads/ffmpeg-ffmpeg-n9.0.2.tar.gz"))
            self.assertEqual(b"diff", archive.read("ports/ffmpeg/patches/0001-fix.patch"))
        first = self.output.read_bytes()
        self.output.unlink()
        bundle_sources.bundle(self.downloads, ["ffmpeg-ffmpeg-n9.0.*.tar.gz"], [self.port], self.output)
        self.assertEqual(first, self.output.read_bytes())

    def test_missing_archive_port_or_existing_output_fail(self):
        for archives, ports in ((["ffmpeg-ffmpeg-n3.3.9.tar.gz"], [self.port]),
                                (["tools/nasm.zip"], [self.port]),
                                (["ffmpeg-*.tar.gz"], [self.root / "ports/missing"])):
            with self.subTest(archives=archives):
                with self.assertRaises(ValueError):
                    bundle_sources.bundle(self.downloads, archives, ports, self.output)
                self.assertFalse(self.output.exists())
                self.assertEqual([], list(self.output.parent.glob("*.partial")))
        self.file("out/ffmpeg-source.zip", b"existing")
        with self.assertRaises(ValueError):
            bundle_sources.bundle(self.downloads, ["ffmpeg-*.tar.gz"], [self.port], self.output)
        self.assertEqual(b"existing", self.output.read_bytes())


if __name__ == "__main__":
    unittest.main()
