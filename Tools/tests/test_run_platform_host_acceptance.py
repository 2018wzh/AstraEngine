import copy
import importlib.util
import pathlib
import sys
import unittest


SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "run_platform_host_acceptance.py"
SPEC = importlib.util.spec_from_file_location("run_platform_host_acceptance", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class PlatformAcceptanceTests(unittest.TestCase):
    def fixture(self):
        package_hash = "sha256:" + "1" * 64
        profile_hash = "sha256:" + "2" * 64
        build_hash = "sha256:" + "3" * 64
        capability = {
            "schema": MODULE.CAPABILITY_SCHEMA,
            "platform": "windows",
            "profile_hash": profile_hash,
            "build_fingerprint": build_hash,
        }
        for domain in ("renderer", "decode", "audio", "save"):
            provider = f"native.{domain}"
            capability[domain] = {
                "selected": provider, "declared": [provider], "available": [provider],
            }
        conformance = {
            "schema": MODULE.CONFORMANCE_SCHEMA,
            "platform": "windows",
            "status": "pass",
            "package_hash": package_hash,
            "profile_hash": profile_hash,
            "build_fingerprint": build_hash,
            "session_id": "native-session",
            "checks": [
                {"id": check_id, "status": "pass", "evidence": [{"key": "result", "value": "measured"}]}
                for check_id in sorted(MODULE.REQUIRED["windows"])
            ],
        }
        player = {
            "schema": MODULE.PLAYER_SCHEMA,
            "status": "pass",
            "package_hash": package_hash,
            "checks": [{
                "id": "player.full_playable", "status": "pass",
                "evidence": [
                    {"key": key, "value": conformance[key]}
                    for key in ("profile_hash", "build_fingerprint", "session_id")
                ],
            }],
        }
        return capability, conformance, player, package_hash

    def validate(self, fixture):
        return MODULE.validate_platform("windows", *fixture)

    def test_matching_native_reports_are_accepted(self):
        self.assertEqual(self.validate(self.fixture())["status"], "pass")

    def test_player_identity_drift_is_blocked(self):
        for key in ("profile_hash", "build_fingerprint", "session_id"):
            with self.subTest(key=key):
                fixture = self.fixture()
                player = fixture[2]
                item = next(item for item in player["checks"][0]["evidence"] if item["key"] == key)
                item["value"] = "different-run"
                report = self.validate(fixture)
                self.assertEqual(report["status"], "blocked")
                self.assertIn(f"player.{key}", report["diagnostics"])

    def test_failed_or_missing_native_input_cannot_pass(self):
        fixture = self.fixture()
        for missing in (False, True):
            with self.subTest(missing=missing):
                changed = copy.deepcopy(fixture)
                checks = changed[1]["checks"]
                check = next(item for item in checks if item["id"] == "input.native_consumption")
                if missing:
                    checks.remove(check)
                else:
                    check["status"] = "blocked"
                report = self.validate(changed)
                self.assertEqual(report["status"], "blocked")
                self.assertIn("conformance.input.native_consumption", report["diagnostics"])

    def test_unavailable_selected_audio_provider_is_blocked(self):
        fixture = self.fixture()
        fixture[0]["audio"]["available"] = []
        report = self.validate(fixture)
        self.assertEqual(report["status"], "blocked")
        self.assertIn("provider.audio", report["diagnostics"])


if __name__ == "__main__":
    unittest.main()
