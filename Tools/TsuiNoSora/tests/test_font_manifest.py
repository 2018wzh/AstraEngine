import re
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from tsuinosora_tools import (  # noqa: E402
    _copy_tsuinosora_ui_font,
    _font_coverage_ranges,
)


def _coverage_from_sidecar(path: Path) -> set[int]:
    coverage = set()
    for start, end in re.findall(r"start: (\d+), end: (\d+)", path.read_text(encoding="utf-8")):
        coverage.update(range(int(start), int(end) + 1))
    return coverage


class TsuiNoSoraFontManifestTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.repository_root = Path(__file__).resolve().parents[3]
        cls.jp_font = (
            cls.repository_root
            / "Examples"
            / "NativeVN"
            / "Assets"
            / "Fonts"
            / "NotoSansJP-Variable.ttf"
        )

    def test_cmap_coverage_reports_only_real_glyphs(self):
        requested = {ord("A"), ord("◆"), ord("僕"), ord("汉"), 0x10FFFF}
        coverage = {
            codepoint
            for start, end in _font_coverage_ranges(self.jp_font, requested)
            for codepoint in range(start, end + 1)
        }
        self.assertTrue({ord("A"), ord("◆"), ord("僕")} <= coverage)
        self.assertNotIn(ord("汉"), coverage)
        self.assertNotIn(0x10FFFF, coverage)

    def test_font_sidecars_use_exact_required_cmap_subsets(self):
        with tempfile.TemporaryDirectory() as directory:
            nativevn_root = Path(directory)
            for relative in ("Localization", "UI", "Scripts", "Controllers", "Themes"):
                (nativevn_root / relative).mkdir(parents=True)
            (nativevn_root / "Localization" / "ja.json").write_text(
                '{"strings":{"line":"僕◆汉　"}}', encoding="utf-8"
            )
            (nativevn_root / "UI" / "classic.astra").write_text(
                'view fixture { text "A" }', encoding="utf-8"
            )

            _copy_tsuinosora_ui_font(self.repository_root, nativevn_root)

            font_root = nativevn_root / "native-assets" / "ui" / "fonts"
            jp_coverage = _coverage_from_sidecar(
                font_root / "NotoSansJP-Variable.ttf.astra-asset.yaml"
            )
            sc_coverage = _coverage_from_sidecar(
                font_root / "NotoSansSC-Variable.ttf.astra-asset.yaml"
            )
            self.assertIn(ord("僕"), jp_coverage)
            self.assertIn(0x3000, jp_coverage)
            self.assertNotIn(ord("汉"), jp_coverage)
            self.assertIn(ord("汉"), sc_coverage)
            self.assertIn("subset: cjk-production-required", (
                font_root / "NotoSansJP-Variable.ttf.astra-asset.yaml"
            ).read_text(encoding="utf-8"))

    def test_truncated_sfnt_fails_fast(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "truncated.ttf"
            path.write_bytes(b"\x00\x01")
            with self.assertRaisesRegex(ValueError, "truncated SFNT header"):
                _font_coverage_ranges(path, {ord("A")})


if __name__ == "__main__":
    unittest.main()
