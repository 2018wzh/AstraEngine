import json
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

from build_private_rc_delivery import DeliveryError, build_delivery, sha256_file


class PrivateRcDeliveryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.bundle = self.root / "bundle"
        (self.bundle / "package").mkdir(parents=True)
        package = self.bundle / "package" / "nativevn.astrapkg"
        package.write_bytes(b"encrypted")
        package_hash = sha256_file(package)
        self.write_json(
            self.bundle / "bundle_manifest.json",
            {
                "schema": "astra.standalone_bundle_manifest.v2",
                "target": "tsuinosora-internal-game",
                "profile": "classic",
                "platform": "windows",
                "package_hash": package_hash,
                "files": [
                    {
                        "path": "package/nativevn.astrapkg",
                        "role": "package",
                        "hash": package_hash,
                        "byte_size": package.stat().st_size,
                    }
                ],
            },
        )
        self.gate = self.root / "gate.json"
        self.write_json(self.gate, self.gate_value(package_hash))

    def tearDown(self) -> None:
        self.temp.cleanup()

    @staticmethod
    def gate_value(package_hash: str) -> dict:
        return {
            "schema": "tsuinosora.private_rc_release_gate.v1",
            "status": "passed",
            "scope": {
                "profile": "classic",
                "locale": "ja",
                "guaranteed_routes": ["Y"],
                "present_unvalidated_route_count": 36,
                "windows_e3": "deferred",
                "distribution": "private_research_preview",
            },
            "identity": {
                "build": "sha256:" + "1" * 64,
                "package": package_hash,
                "headless_report": "sha256:" + "2" * 64,
                "y_route_report": "sha256:" + "3" * 64,
                "comparison_report": "sha256:" + "4" * 64,
                "manual_signoff": "sha256:" + "5" * 64,
            },
            "counts": {"visual_checks_required": 13, "visual_checks_passed": 13},
            "checks": [{"id": "all", "status": "pass"}],
            "diagnostics": [],
        }

    @staticmethod
    def write_json(path: Path, value: object) -> None:
        path.write_text(json.dumps(value), encoding="utf-8")

    def test_passed_gate_builds_bounded_private_delivery(self) -> None:
        output = self.root / "delivery"
        report = build_delivery(
            self.bundle,
            self.gate,
            output,
            datetime(2026, 7, 20, tzinfo=timezone.utc),
            7,
        )
        self.assertEqual(report["status"], "ready_for_private_distribution")
        self.assertEqual(report["expires_at"], "2026-07-27T00:00:00Z")
        payload = output / "TsuiNoSora-Classic-Private-RC"
        self.assertTrue((payload / "PRIVATE_RESEARCH_NOTICE.txt").is_file())
        self.assertTrue((payload / "private-rc-delivery-manifest.json").is_file())
        self.assertNotIn(str(self.root), json.dumps(report))

    def test_blocked_gate_cannot_build_delivery(self) -> None:
        value = json.loads(self.gate.read_text(encoding="utf-8"))
        value["status"] = "blocked"
        self.write_json(self.gate, value)
        with self.assertRaisesRegex(DeliveryError, "release gate is not passed"):
            build_delivery(
                self.bundle,
                self.gate,
                self.root / "delivery",
                datetime(2026, 7, 20, tzinfo=timezone.utc),
                7,
            )

    def test_tampered_bundle_and_non_seven_day_retention_fail(self) -> None:
        (self.bundle / "package" / "nativevn.astrapkg").write_bytes(b"tampered")
        with self.assertRaisesRegex(DeliveryError, "integrity failed"):
            build_delivery(
                self.bundle,
                self.gate,
                self.root / "delivery-a",
                datetime(2026, 7, 20, tzinfo=timezone.utc),
                7,
            )
        with self.assertRaisesRegex(DeliveryError, "seven days"):
            build_delivery(
                self.bundle,
                self.gate,
                self.root / "delivery-b",
                datetime(2026, 7, 20, tzinfo=timezone.utc),
                6,
            )


if __name__ == "__main__":
    unittest.main()
