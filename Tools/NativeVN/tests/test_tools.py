from __future__ import annotations

import json
import os
import struct
import sys
import tempfile
import unittest
import wave
import zlib
from pathlib import Path
from unittest import mock


TOOLS_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS_ROOT))

from audio_catalog import AUDIO_SPECS, AudioSpec  # noqa: E402
from build_contact_sheet import build_contact_sheet  # noqa: E402
from common import ToolFailure, sha256_file  # noqa: E402
from generate_audio import render_wav  # noqa: E402
from generate_voice import _validate_cues, _validate_voice_metadata, generate_voice  # noqa: E402
from review_audio_openrouter import _parse_content, review as review_audio_openrouter  # noqa: E402
from update_manifest import update_audio_manifest  # noqa: E402
from validate_content_pack import ContentPackValidator, inspect_png, inspect_wav  # noqa: E402


def write_png(path: Path, width: int = 4, height: int = 3, *, alpha: bool = True, chroma: bool = False) -> None:
    color_type = 6 if alpha else 2
    channels = 4 if alpha else 3
    pixel = (0, 255, 0) if chroma else (40, 80, 120)
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            rows.extend(pixel)
            if alpha:
                rows.append(0 if x == 0 and y == 0 else 255)
    ihdr = struct.pack(">IIBBBBB", width, height, 8, color_type, 0, 0, 0)

    def chunk(kind: bytes, payload: bytes) -> bytes:
        return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(rows))) + chunk(b"IEND", b""))


def write_pcm24(path: Path, frames: int = 4_800, amplitude: int = 700_000) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = bytearray()
    for index in range(frames):
        value = amplitude if index % 2 else -amplitude
        unsigned = value & 0xFFFFFF
        packed = bytes((unsigned & 0xFF, (unsigned >> 8) & 0xFF, (unsigned >> 16) & 0xFF))
        payload.extend(packed)
        payload.extend(packed)
    with wave.open(str(path), "wb") as stream:
        stream.setnchannels(2)
        stream.setsampwidth(3)
        stream.setframerate(48_000)
        stream.writeframes(payload)


class AudioCatalogTests(unittest.TestCase):
    def test_catalog_shape_and_duration_contracts(self) -> None:
        self.assertEqual(sum(item.kind == "bgm" for item in AUDIO_SPECS), 4)
        self.assertEqual(sum(item.kind == "stinger" for item in AUDIO_SPECS), 3)
        self.assertEqual(sum(item.kind == "se" for item in AUDIO_SPECS), 18)
        for item in AUDIO_SPECS:
            bounds = {"bgm": (60, 90), "stinger": (5, 9), "se": (0.2, 8)}[item.kind]
            self.assertTrue(bounds[0] <= item.duration_seconds <= bounds[1])

    def test_renderer_is_deterministic_and_writes_pcm24_stereo(self) -> None:
        spec = AudioSpec("test", "se", 0.025, 123, "Test", "测试", "terminal_key")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            first = root / "first.wav"
            second = root / "second.wav"
            render_wav(spec, first)
            render_wav(spec, second)
            self.assertEqual(sha256_file(first), sha256_file(second))
            info = inspect_wav(first)
            self.assertEqual((info.sample_rate, info.sample_width_bits, info.channels), (48_000, 24, 2))
            self.assertGreater(info.peak, 0.001)

    def test_manifest_records_hashes_and_sizes(self) -> None:
        spec = AUDIO_SPECS[0]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            wav_path = root / "Master" / spec.kind / f"{spec.asset_id}.wav"
            ogg_path = root / "Distribution" / spec.kind / f"{spec.asset_id}.ogg"
            write_pcm24(wav_path)
            ogg_path.parent.mkdir(parents=True)
            ogg_path.write_bytes(b"OggS" + b"test-distribution")
            manifest = update_audio_manifest(root, require_complete=False)
            self.assertEqual(manifest["asset_count"], 1)
            entry = manifest["assets"][0]
            self.assertEqual(entry["master"]["sha256"], sha256_file(wav_path))
            self.assertEqual(entry["distribution"]["byte_size"], ogg_path.stat().st_size)


class VoiceTests(unittest.TestCase):
    def test_missing_key_fails_without_creating_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary, mock.patch.dict(os.environ, {}, clear=True):
            root = Path(temporary)
            with self.assertRaises(ToolFailure) as raised:
                generate_voice(root)
            self.assertEqual(raised.exception.code, "NATIVEVN_ELEVENLABS_API_KEY_MISSING")
            self.assertFalse((root / ".local").exists())

    def test_clone_fields_are_rejected(self) -> None:
        payload = {
            "schema": "astra.nativevn_flagship.voice_cues.v1",
            "cues": [{"id": "narrator-1", "voice_id": "LibraryVoice123", "text": "hello", "voice_clone": True}],
        }
        with self.assertRaises(ToolFailure) as raised:
            _validate_cues(payload)
        self.assertEqual(raised.exception.code, "NATIVEVN_VOICE_CLONING_FORBIDDEN")

    def test_remote_cloned_voice_metadata_is_rejected(self) -> None:
        with self.assertRaises(ToolFailure) as raised:
            _validate_voice_metadata({"category": "cloned"})
        self.assertEqual(raised.exception.code, "NATIVEVN_VOICE_CLONING_FORBIDDEN")

    def test_bilingual_cue_selects_requested_locale(self) -> None:
        payload = {
            "schema": "astra.nativevn_flagship.voice_cues.v1",
            "cues": [{"id": "narrator-1", "voice_id": "LibraryVoice123", "text": {"zh": "你好", "en": "Hello"}, "locale": "zh"}],
        }
        cues = _validate_cues(payload)
        self.assertEqual(cues[0]["resolved_text"], "你好")

    def test_content_schema_cue_resolves_screenplay_line_and_profile(self) -> None:
        payload = {
            "schema": "nativevn.flagship_voice_cues.v1",
            "package_id": "nativevn.flagship_content",
            "voice_profiles": {"lin_yao": {"voice_id": "LibraryVoice123", "voice_source": "elevenlabs_library"}},
            "cues": [{"id": "vc_line_1", "speaker_id": "lin_yao", "line_id": "line_1"}],
        }
        cues = _validate_cues(payload, line_texts={"line_1": "线路已经稳定。"})
        self.assertEqual(cues[0]["resolved_text"], "线路已经稳定。")
        self.assertEqual(cues[0]["voice_id"], "LibraryVoice123")

    def test_narrative_schema_uses_actor_elevenlabs_voice_id(self) -> None:
        payload = {
            "schema": "astra.nativevn.flagship.voice_cues.v1",
            "story_id": "signal_in_the_glass_rain",
            "actors": [{"speaker_id": "lin_yao", "elevenlabs_voice_id": "LibraryVoice123"}],
            "cues": [{"id": "vc_line_1", "speaker_id": "lin_yao", "line_id": "line_1"}],
        }
        cues = _validate_cues(payload, line_texts={"line_1": "线路已经稳定。"})
        self.assertEqual(cues[0]["voice_id"], "LibraryVoice123")


class OpenRouterAudioReviewTests(unittest.TestCase):
    def test_provider_aliases_are_explicitly_normalized(self) -> None:
        result = _parse_content(json.dumps({"verdict": "pass", "reason": "clean and suitable"}))
        self.assertEqual(result["decision"], "pass")
        self.assertTrue(result["fit_for_role"])
        self.assertIn("verdict_to_decision", result["contract_normalizations"])

    def test_missing_key_fails_before_reading_or_writing_assets(self) -> None:
        with tempfile.TemporaryDirectory() as temporary, mock.patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(ToolFailure) as raised:
                review_audio_openrouter(Path(temporary))
            self.assertEqual(raised.exception.code, "NATIVEVN_OPENROUTER_API_KEY_MISSING")

class PngTests(unittest.TestCase):
    def test_png_alpha_and_chroma_are_inspected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            normal = Path(temporary) / "normal.png"
            chroma = Path(temporary) / "chroma.png"
            write_png(normal)
            write_png(chroma, chroma=True)
            normal_info = inspect_png(normal)
            chroma_info = inspect_png(chroma)
            self.assertTrue(normal_info.has_alpha)
            self.assertEqual(normal_info.transparent_pixels, 1)
            self.assertGreater(chroma_info.chroma_pixels, 0)

    def test_contact_sheet_stays_in_local_review(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            write_png(root / "Visual" / "sample.png", width=32, height=24)
            output = root / ".local" / "review" / "sheet.png"
            try:
                count = build_contact_sheet(root, output, columns=1, cell_width=128, cell_height=128)
            except ToolFailure as error:
                if error.code == "NATIVEVN_PILLOW_MISSING":
                    self.skipTest("optional Pillow is not installed")
                raise
            self.assertEqual(count, 1)
            self.assertTrue(output.is_file())


class ValidatorTests(unittest.TestCase):
    def test_full_screen_ui_mockup_does_not_require_alpha(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            write_png(root / "Visual" / "UI" / "title.png", alpha=False)
            diagnostics, _summary = ContentPackValidator(root).validate()
            self.assertFalse(any(item.code == "NATIVEVN_IMAGE_ALPHA_REQUIRED" for item in diagnostics.items))

    def test_development_mode_warns_for_not_yet_generated_sections(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            diagnostics, _summary = ContentPackValidator(Path(temporary), release=False).validate()
            self.assertFalse(diagnostics.failed)
            self.assertTrue(any(item.code == "NATIVEVN_VISUAL_ASSETS_MISSING" for item in diagnostics.items))

    def test_release_mode_blocks_missing_sections(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            diagnostics, _summary = ContentPackValidator(Path(temporary), release=True).validate()
            self.assertTrue(diagnostics.failed)
            self.assertTrue(any(item.code == "NATIVEVN_AUDIO_CATALOG_INCOMPLETE" for item in diagnostics.items))

    def test_release_mode_accepts_user_authorized_voice_status(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = root / "Manifests" / "content-manifest.json"
            manifest.parent.mkdir(parents=True)
            manifest.write_text(json.dumps({
                "schema": "nativevn.flagship_content_manifest.v1",
                "package_id": "nativevn.flagship_content",
                "status": {"public_release_assets": "ready_with_authorized_voice"},
                "assets": [],
            }), encoding="utf-8")
            diagnostics, _summary = ContentPackValidator(root, release=True).validate()
            self.assertFalse(any(item.code == "NATIVEVN_VOICE_RIGHTS_BLOCKED" for item in diagnostics.items))

    def test_project_descriptor_is_allowed_for_cook_milestone(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "project.yaml").write_text("id: flagship\n", encoding="utf-8")
            diagnostics, _summary = ContentPackValidator(root).validate()
            self.assertFalse(any(item.code == "NATIVEVN_FORBIDDEN_RUNTIME_FILE" for item in diagnostics.items))

    def test_runtime_scenario_is_out_of_scope_for_cook_milestone(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "route.scenario.yaml").write_text("schema: test\n", encoding="utf-8")
            diagnostics, _summary = ContentPackValidator(root).validate()
            self.assertTrue(any(item.code == "NATIVEVN_RUNTIME_TEST_SCOPE_FORBIDDEN" for item in diagnostics.items))

    def test_absolute_paths_and_secrets_are_blocking(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            synthetic_absolute = "C" + ":" + "\\Users\\person\\asset"
            synthetic_secret = "sk" + "-" + "1234567890abcdefghijklmnop"
            (root / "notes.txt").write_text(f"path={synthetic_absolute} key={synthetic_secret}\n", encoding="utf-8")
            diagnostics, _summary = ContentPackValidator(root).validate()
            codes = {item.code for item in diagnostics.items}
            self.assertIn("NATIVEVN_ABSOLUTE_PATH_FORBIDDEN", codes)
            self.assertIn("NATIVEVN_SECRET_FORBIDDEN", codes)

    def test_manifest_mismatch_is_blocking(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            asset = root / "Audio" / "sample.bin"
            asset.parent.mkdir(parents=True)
            asset.write_bytes(b"actual")
            manifest = {
                "schema": "astra.nativevn_flagship.test_manifest.v1",
                "id": "test-manifest",
                "asset_count": 1,
                "assets": [{"id": "sample", "path": "sample.bin", "sha256": "0" * 64, "byte_size": 999}],
            }
            (root / "Audio" / "test-manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
            diagnostics, _summary = ContentPackValidator(root).validate()
            codes = {item.code for item in diagnostics.items}
            self.assertIn("NATIVEVN_MANIFEST_HASH_MISMATCH", codes)
            self.assertIn("NATIVEVN_MANIFEST_SIZE_MISMATCH", codes)

    def test_content_manifest_relative_locator_is_verified_from_pack_root(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            asset = root / "Visual" / "asset.bin"
            asset.parent.mkdir(parents=True)
            asset.write_bytes(b"content")
            manifest = {
                "schema": "nativevn.flagship_content_manifest.v1",
                "package_id": "nativevn.flagship_content",
                "asset_count": 1,
                "assets": [{
                    "id": "visual_asset",
                    "locator": {"relative_path": "Visual/asset.bin"},
                    "sha256": sha256_file(asset),
                    "byte_size": asset.stat().st_size,
                }],
            }
            manifest_path = root / "Manifests" / "content-manifest.json"
            manifest_path.parent.mkdir(parents=True)
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            diagnostics, _summary = ContentPackValidator(root).validate()
            codes = {item.code for item in diagnostics.items}
            self.assertNotIn("NATIVEVN_MANIFEST_HASH_MISMATCH", codes)
            self.assertNotIn("NATIVEVN_MANIFEST_SIZE_MISMATCH", codes)

    def test_wav_silence_is_blocking(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            wav_path = root / "Audio" / "Master" / "se" / "silent.wav"
            write_pcm24(wav_path, amplitude=0)
            diagnostics, _summary = ContentPackValidator(root).validate()
            self.assertTrue(any(item.code == "NATIVEVN_WAV_SILENT" for item in diagnostics.items))


if __name__ == "__main__":
    unittest.main()
