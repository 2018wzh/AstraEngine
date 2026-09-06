import json
import pathlib
import sys
import tempfile
import unittest
from unittest import mock


TOOLS_DIR = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS_DIR))

import build_astraemu_desktop


class AstraEmuDesktopPackageTests(unittest.TestCase):
    def test_ephemeral_signer_never_reuses_environment_secret(self):
        environment = {}
        with mock.patch("build_astraemu_desktop.secrets.token_hex", return_value="ab" * 32):
            build_astraemu_desktop.configure_signer(
                pathlib.Path.cwd(), environment, True, False, None
            )
        self.assertEqual(environment["ASTRA_EMU_FAMILY_SIGNING_KEY_HEX"], "ab" * 32)
        self.assertEqual(environment["ASTRA_EMU_FAMILY_SIGNER_ID"], "astra.development.local")
        self.assertNotIn("ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX", environment)

    def test_release_signer_requires_process_environment(self):
        with self.assertRaisesRegex(SystemExit, "ASTRA_EMU_DESKTOP_SIGNER_ENV_MISSING"):
            build_astraemu_desktop.configure_signer(
                pathlib.Path.cwd(), {}, False, False, None
            )

    def test_windows_runtime_requires_static_msvc_crt(self):
        environment = {"RUSTFLAGS": "-C debuginfo=1"}
        build_astraemu_desktop.configure_windows_runtime(
            "x86_64-pc-windows-msvc", environment
        )
        self.assertIn("-C debuginfo=1", environment["RUSTFLAGS"])
        self.assertIn("target-feature=+crt-static", environment["RUSTFLAGS"])
        self.assertNotIn("CARGO_ENCODED_RUSTFLAGS", environment)

    def test_windows_runtime_keeps_existing_static_flag_and_rejects_dynamic(self):
        environment = {"RUSTFLAGS": "-C target-feature=+crt-static"}
        build_astraemu_desktop.configure_windows_runtime(
            "x86_64-pc-windows-msvc", environment
        )
        self.assertEqual(environment["RUSTFLAGS"].count("crt-static"), 1)
        with self.assertRaisesRegex(SystemExit, "ASTRA_EMU_DESKTOP_CRT_POLICY_CONFLICT"):
            build_astraemu_desktop.configure_windows_runtime(
                "x86_64-pc-windows-msvc",
                {"RUSTFLAGS": "-C target-feature=-crt-static"},
            )

    def test_windows_runtime_updates_encoded_flags(self):
        environment = {"CARGO_ENCODED_RUSTFLAGS": "-C\x1ftarget-cpu=haswell"}
        build_astraemu_desktop.configure_windows_runtime(
            "x86_64-pc-windows-msvc", environment
        )
        self.assertIn("-C\x1ftarget-feature=+crt-static", environment["CARGO_ENCODED_RUSTFLAGS"])

    def test_non_windows_runtime_is_unchanged(self):
        environment = {"RUSTFLAGS": "-C target-feature=-crt-static"}
        build_astraemu_desktop.configure_windows_runtime(
            "x86_64-unknown-linux-gnu", environment
        )
        self.assertEqual(environment["RUSTFLAGS"], "-C target-feature=-crt-static")

    def test_musica_distribution_compiles_only_the_explicit_video_provider(self):
        self.assertEqual(
            build_astraemu_desktop.desktop_features("musica", "ffmpeg-vcpkg"),
            (
                "astra-emu-musica/dynamic-plugin-export",
                "astra-emu-manager/ffmpeg-vcpkg",
                "astra-emu-cli/ffmpeg-vcpkg",
            ),
        )
        self.assertEqual(
            build_astraemu_desktop.desktop_features("musica", "wmf"),
            ("astra-emu-musica/dynamic-plugin-export",),
        )
        self.assertEqual(
            build_astraemu_desktop.desktop_features("fvp", None),
            ("astra-emu-fvp/dynamic-plugin-export",),
        )

    def test_evidence_redaction_rejects_absolute_paths_recursively(self):
        self.assertTrue(build_astraemu_desktop.has_absolute_path({"nested": ["C:\\private\\game"]}))
        self.assertTrue(build_astraemu_desktop.has_absolute_path({"nested": ["/private/game"]}))
        self.assertFalse(
            build_astraemu_desktop.has_absolute_path(
                {"family_file": "families/fvp/astra_emu_fvp.dll", "hash": "sha256." + "a" * 64}
            )
        )

    def test_build_identity_shape_contains_no_path_field(self):
        value = {
            "schema": "astra.build_identity.v1",
            "identity_id": "0" * 16,
            "commit": "0" * 40,
            "worktree_state": "dirty",
            "source_state_sha256": "sha256." + "0" * 64,
            "cargo_lock_sha256": "sha256." + "1" * 64,
            "rust_toolchain_sha256": "sha256." + "2" * 64,
            "target": "x86_64-pc-windows-msvc",
            "profile": "release",
        }
        self.assertFalse(build_astraemu_desktop.has_absolute_path(value))
        self.assertNotIn("path", json.dumps(value).lower())

    def test_family_library_name_is_target_and_family_specific(self):
        self.assertEqual(
            build_astraemu_desktop.family_library_name("musica", "x86_64-pc-windows-msvc"),
            "astra_emu_musica.dll",
        )
        self.assertEqual(
            build_astraemu_desktop.family_library_name("fvp", "x86_64-unknown-linux-gnu"),
            "libastra_emu_fvp.so",
        )

    def test_copy_notice_uses_an_explicit_family_name_and_rejects_collisions(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            source = root / "THIRD_PARTY_NOTICES.md"
            source.write_text("notice", encoding="utf-8")
            output = root / "output"
            output.mkdir()
            build_astraemu_desktop.copy_notice(source, output, "MUSICA_THIRD_PARTY_NOTICES.md")
            self.assertEqual((output / "MUSICA_THIRD_PARTY_NOTICES.md").read_text(encoding="utf-8"), "notice")
            with self.assertRaises(SystemExit) as raised:
                build_astraemu_desktop.copy_notice(source, output, "MUSICA_THIRD_PARTY_NOTICES.md")
            self.assertEqual(str(raised.exception), "ASTRA_EMU_DESKTOP_NOTICE_COLLISION")


if __name__ == "__main__":
    unittest.main()
