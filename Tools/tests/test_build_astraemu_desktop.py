import json
import pathlib
import sys
import unittest
from unittest import mock


TOOLS_DIR = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS_DIR))

import build_astraemu_desktop


class AstraEmuDesktopPackageTests(unittest.TestCase):
    def test_ephemeral_signer_never_reuses_environment_secret(self):
        environment = {}
        with mock.patch("build_astraemu_desktop.secrets.token_hex", return_value="ab" * 32):
            build_astraemu_desktop.configure_signer(environment, True, None)
        self.assertEqual(environment["ASTRA_EMU_FAMILY_SIGNING_KEY_HEX"], "ab" * 32)
        self.assertEqual(environment["ASTRA_EMU_FAMILY_SIGNER_ID"], "astra.development.local")
        self.assertNotIn("ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX", environment)

    def test_release_signer_requires_process_environment(self):
        with self.assertRaisesRegex(SystemExit, "ASTRA_EMU_DESKTOP_SIGNER_ENV_MISSING"):
            build_astraemu_desktop.configure_signer({}, False, None)

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


if __name__ == "__main__":
    unittest.main()
