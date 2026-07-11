import hashlib
import json
import pathlib
import tempfile
import unittest

import sys

TOOLS_DIR = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS_DIR))

import run_cargo_isolated


class CargoIsolationTests(unittest.TestCase):
    def test_identity_binds_checkout_manifest_lock_toolchain_and_features(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "Cargo.toml").write_text("[workspace]\nmembers=[]\n", encoding="utf-8")
            (root / "Cargo.lock").write_text("version = 4\n", encoding="utf-8")
            member = root / "Engine" / "member"
            member.mkdir(parents=True)
            (member / "Cargo.toml").write_text(
                "[package]\nname='member'\nversion='0.1.0'\n",
                encoding="utf-8",
            )

            identity = run_cargo_isolated.build_identity(
                root=root,
                git_head="abc123",
                git_diff=b"dirty-change",
                rustc_version="rustc 1.90.0\nhost: x86_64-pc-windows-msvc",
                cargo_args=["test", "--workspace", "--features", "desktop-wgpu,desktop-audio"],
            )

            self.assertEqual(identity["schema"], "astra.build_identity.v1")
            self.assertEqual(identity["checkout_id"], "abc123")
            self.assertTrue(identity["workspace_manifest_hash"].startswith("sha256:"))
            self.assertEqual(
                identity["dependency_lock_hash"],
                "sha256:"
                + hashlib.sha256((root / "Cargo.lock").read_bytes()).hexdigest(),
            )
            self.assertTrue(identity["toolchain_fingerprint"].startswith("sha256:"))
            self.assertTrue(identity["feature_fingerprint"].startswith("sha256:"))
            self.assertTrue(identity["checkout_state_hash"].startswith("sha256:"))
            self.assertNotIn(str(root), json.dumps(identity))

    def test_target_directory_is_derived_from_complete_identity(self):
        identity = {
            "identity_hash": "sha256:0123456789abcdef" + "0" * 48,
        }
        target = run_cargo_isolated.target_directory(pathlib.Path("workspace"), identity)
        self.assertEqual(
            target,
            pathlib.Path("workspace") / "target" / "identity" / "0123456789abcdef",
        )

    def test_untracked_source_content_changes_checkout_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "Cargo.toml").write_text("[workspace]\nmembers=[]\n", encoding="utf-8")
            (root / "Cargo.lock").write_text("version = 4\n", encoding="utf-8")
            common = dict(
                root=root,
                git_head="abc123",
                git_diff=b"",
                rustc_version="rustc 1.90.0",
                cargo_args=["test"],
            )

            first = run_cargo_isolated.build_identity(
                **common, untracked_files=[("Engine/new.rs", b"first")]
            )
            second = run_cargo_isolated.build_identity(
                **common, untracked_files=[("Engine/new.rs", b"second")]
            )

            self.assertNotEqual(first["checkout_state_hash"], second["checkout_state_hash"])
            self.assertNotEqual(first["identity_hash"], second["identity_hash"])

    def test_generated_target_manifests_do_not_change_workspace_manifest_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "Cargo.toml").write_text("[workspace]\nmembers=[]\n", encoding="utf-8")
            (root / "Cargo.lock").write_text("version = 4\n", encoding="utf-8")
            arguments = dict(
                root=root,
                git_head="abc123",
                git_diff=b"",
                rustc_version="rustc 1.90.0",
                cargo_args=["test"],
            )
            before = run_cargo_isolated.build_identity(**arguments)
            generated = root / "target" / "identity" / "generated"
            generated.mkdir(parents=True)
            (generated / "Cargo.toml").write_text("generated=true\n", encoding="utf-8")
            after = run_cargo_isolated.build_identity(**arguments)

            self.assertEqual(
                before["workspace_manifest_hash"], after["workspace_manifest_hash"]
            )

    def test_artifact_manifest_records_only_relative_binary_roles(self):
        with tempfile.TemporaryDirectory() as directory:
            target = pathlib.Path(directory)
            profile = target / "debug"
            deps = profile / "deps"
            deps.mkdir(parents=True)
            (profile / "AstraPlayer.exe").write_bytes(b"player")
            (profile / "fixture.dll").write_bytes(b"plugin")
            (profile / "notes.txt").write_text("ignore", encoding="utf-8")
            (deps / "test-helper.exe").write_bytes(b"ignore dependency artifact")

            artifacts = run_cargo_isolated.collect_artifacts(target)

            self.assertEqual(
                [(item["path"], item["role"]) for item in artifacts],
                [
                    ("debug/AstraPlayer.exe", "executable"),
                    ("debug/fixture.dll", "dynamic_library"),
                ],
            )
            self.assertTrue(all(item["sha256"].startswith("sha256:") for item in artifacts))
            self.assertTrue(all(item["byte_size"] > 0 for item in artifacts))

    def test_existing_target_identity_mismatch_is_blocking(self):
        with tempfile.TemporaryDirectory() as directory:
            report = pathlib.Path(directory) / "astra-build-identity.json"
            report.write_text(
                json.dumps({"identity_hash": "sha256:" + "1" * 64}),
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ValueError, "ASTRA_BUILD_IDENTITY_MISMATCH"):
                run_cargo_isolated.validate_existing_identity(
                    report,
                    {"identity_hash": "sha256:" + "2" * 64},
                )

    def test_checkout_change_during_cargo_is_blocking(self):
        before = {"identity_hash": "sha256:" + "1" * 64}
        after = {"identity_hash": "sha256:" + "2" * 64}

        with self.assertRaisesRegex(ValueError, "ASTRA_BUILD_INPUT_CHANGED"):
            run_cargo_isolated.ensure_identity_unchanged(before, after)


if __name__ == "__main__":
    unittest.main()
