from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_nativevn_project as project


class ProjectRefreshTests(unittest.TestCase):
    def test_refresh_preserves_authored_astra_and_controller_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            authored = {
                "Scripts/main.astra": b"# author comment\nstory custom #@id stable.story\n",
                "Scripts/system.astra": b"# system actions remain authored\n",
                "UI/flagship.astra": b"# authored view\n",
                "Controllers/standard_ui.luau": b"-- authored controller\n",
                "Themes/classic.json": b"{}\n",
                "Themes/modern.json": b"{}\n",
            }
            for name, content in authored.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(content)
            with mock.patch.object(project, "PACK", root), \
                 mock.patch.object(project, "copy_fonts"), \
                 mock.patch.object(project, "build_localization"), \
                 mock.patch.object(project, "build_sidecars"):
                self.assertEqual(project.main(), 0)
            for name, content in authored.items():
                self.assertEqual((root / name).read_bytes(), content, name)
            self.assertEqual(
                (root / "project.yaml").read_text(),
                (project.PACK / "project.yaml").read_text(),
                "refresh must not resurrect the obsolete platform descriptor",
            )


if __name__ == "__main__":
    unittest.main()
