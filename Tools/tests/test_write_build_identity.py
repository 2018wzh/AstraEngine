import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "write_build_identity", Path(__file__).resolve().parents[1] / "write_build_identity.py"
)
identity = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(identity)


class BuildIdentityTests(unittest.TestCase):
    def test_native_build_does_not_require_unrelated_web_toolchain(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.lock").write_bytes(b"version = 4\n")
            (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            (root / "Tools").mkdir()
            (root / "Tools/ui-toolchain-lock.json").write_text("{}", encoding="utf-8")
            with (
                patch.object(identity, "run_output", side_effect=[b"abc123\n", b"", b"rustc test"]),
                patch.object(identity, "untracked_files", return_value=[]),
            ):
                result = identity.build_identity(root, ["build", "--release", "-p", "astra-headless"])
            self.assertFalse(result["dirty"])
            self.assertEqual(result["checkout_id"], "abc123")
            self.assertEqual(
                result["dependency_lock_hash"], identity.sha256(b"version = 4\n")
            )
            self.assertNotIn("ui_toolchain", result)
            claimed = result.pop("identity_hash")
            self.assertEqual(
                claimed,
                identity.sha256(json.dumps(result, sort_keys=True, separators=(",", ":")).encode()),
            )


if __name__ == "__main__":
    unittest.main()
