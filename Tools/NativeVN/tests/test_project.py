from pathlib import Path
import sys
import re
import shutil
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_nativevn_project as project


class ProjectRefreshTests(unittest.TestCase):
    def test_runtime_media_and_font_sidecars_regenerate_without_drift(self):
        fonts = {
            f"Assets/Fonts/NotoSans{language}-Variable.ttf": f"font__notosans{language.lower()}-variable.astra-asset.yaml"
            for language in ("JP", "SC")
        }
        expected_fonts = {
            name: (project.PACK / "AssetSidecars" / name).read_bytes() for name in fonts.values()
        }
        media = {
            "Audio/Master/bgm/theme.wav": ("bgm/theme", "audio.wav"),
            "Audio/Master/se/rain.wav": ("se/rain", "audio.wav"),
            "Audio/Master/stinger/chime.wav": ("stinger/chime", "audio.wav"),
            "Audio/Voice/Master/speaker/line.wav": ("voice/line", "audio.wav"),
            "Visual/Video/rain-signal-loop.mp4": ("movie/rain-signal-loop", "video.mp4"),
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in media:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"media-source")
            for source in fonts:
                (root / source).parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(project.PACK / source, root / source)
            with mock.patch.object(project, "PACK", root):
                self.assertEqual(set(project.discover_runtime_assets()), {*media, *fonts})
                project.build_sidecars()
                sidecars = root / "AssetSidecars"
                first = {p.name: p.read_bytes() for p in sidecars.iterdir()}
                for name, data in expected_fonts.items():
                    self.assertEqual(first[name], data)
                for source, (logical_id, kind) in media.items():
                    name = logical_id.replace("/", "__") + ".astra-asset.yaml"
                    text = first[name].decode("utf-8")
                    self.assertIn(f"id: asset:/{logical_id}\n", text)
                    self.assertIn(f"source: {source}\n", text)
                    self.assertIn(f"type: {kind}\n", text)
                project.build_sidecars()
                self.assertEqual(first, {p.name: p.read_bytes() for p in sidecars.iterdir()})

    def test_authored_dialogue_windows_resolve_in_every_ui_profile(self):
        # Player resolves MessageViewModel.window as the surface name. Cook alone
        # does not reject a story referencing an unbound window.
        windows = set()
        for source in (project.PACK / "Scripts").glob("*.astra"):
            for line in source.read_text(encoding="utf-8").splitlines():
                if line.strip().startswith("text "):
                    match = re.search(r"\bwindow:([\w.-]+)", line)
                    windows.add(match.group(1) if match else "message")
        profiles = set()
        surfaces = {}
        for line in (project.PACK / "UI/flagship.astra").read_text(encoding="utf-8").splitlines():
            attrs = dict(re.findall(r"([a-z_]+):([\w.-]+)", line))
            if line.startswith("ui_policy "):
                profiles.add(attrs["profile"])
            if line.startswith("ui_bind ") and "surface" in attrs:
                surfaces.setdefault(attrs.get("profile"), set()).add(attrs["surface"])
        self.assertTrue(windows)
        self.assertTrue(profiles)
        for profile in profiles:
            missing = windows - surfaces.get(None, set()) - surfaces.get(profile, set())
            self.assertFalse(missing, f"{profile}: unbound dialogue windows {sorted(missing)}")

    def test_message_views_bind_authoritative_reveal_progress(self):
        source = (project.PACK / "UI/flagship.astra").read_text(encoding="utf-8")
        bodies = [line for line in source.splitlines() if "text id:body value:$model.text_key" in line]
        self.assertEqual(len(bodies), 2)
        for body in bodies:
            self.assertIn("visible_graphemes:$model.visible_graphemes", body)

    def test_refresh_preserves_authored_astra_and_controller_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            authored = {
                "Scripts/main.astra": b"# author comment\nstory custom #@id stable.story\n",
                "Scripts/system.astra": b"# system actions remain authored\n",
                "Scripts/experience.astra": b"# independently authored layered performance\n",
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
                (root / "project.yaml").read_text(encoding="utf-8"),
                (project.PACK / "project.yaml").read_text(encoding="utf-8"),
                "refresh must not resurrect the obsolete platform descriptor",
            )


if __name__ == "__main__":
    unittest.main()
