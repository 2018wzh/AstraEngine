import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest


SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "run_platform_host_acceptance.py"
SPEC = importlib.util.spec_from_file_location("run_platform_host_acceptance", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def write_json(path: pathlib.Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, sort_keys=True) + "\n", encoding="utf-8")


class PlatformAcceptancePreflightTests(unittest.TestCase):
    def fixture(self, root: pathlib.Path) -> dict[str, object]:
        package = root / "game.astrapkg"
        package.write_bytes(b"package")
        player = root / "player.json"
        write_json(player, {"schema": MODULE.PLAYER_SCHEMA, "status": "pass"})
        run = root / "headless-run.json"
        run_payload = {
            "schema": MODULE.HEADLESS_RUN_SCHEMA,
            "status": "passed",
            "build_fingerprint": MODULE.sha256(package),
            "package_hash": MODULE.sha256(package),
            "input_sequence_hash": "sha256:" + "1" * 64,
            "profile_id": "headless-profile",
            "session_id": "headless-session",
            "scenario": "full-route",
            "target": "native-vn",
            "content_identity": "public-content",
            "manifest_hash": "sha256:" + "2" * 64,
            "checkpoint_results": [{"id": "final", "passed": True}],
        }
        write_json(run, run_payload)
        bundle = root / "review-bundle.json"
        write_json(
            bundle,
            {
                "schema": MODULE.HEADLESS_REVIEW_BUNDLE_SCHEMA,
                "run_report_hash": MODULE.sha256(run),
                "manifest_hash": run_payload["manifest_hash"],
                "automatic_passed": True,
                "selected_frames": [{"relative_path": "frames/final.png"}],
                "selected_audio": [{"relative_path": "audio/full.wav"}],
                "required_checkpoints": ["final"],
            },
        )
        review = root / "review.json"
        write_json(
            review,
            {
                "schema": MODULE.HEADLESS_REVIEW_SCHEMA,
                "run_report_hash": MODULE.sha256(run),
                "reviewer_kind": "model",
                "reviewer_identity": "codex-visual-audio-review",
                "tool_identity_hash": "sha256:" + "3" * 64,
                "checkpoints": [
                    {"checkpoint": "final", "passed": True, "diagnostic_codes": []}
                ],
            },
        )
        conformance = {
            "profile_hash": "profile-v2",
            "session_id": "platform-session",
        }
        identity = root / "platform-identity.json"
        identity_payload = {
            "schema": MODULE.PLATFORM_RUN_IDENTITY_SCHEMA,
            "run_report_hash": MODULE.sha256(player),
            "build_fingerprint": run_payload["build_fingerprint"],
            "cooked_package_hash": run_payload["package_hash"],
            "input_sequence_hash": run_payload["input_sequence_hash"],
            "scenario": run_payload["scenario"],
            "target": run_payload["target"],
            "content_identity": run_payload["content_identity"],
            "profile_id": conformance["profile_hash"],
            "session_id": conformance["session_id"],
        }
        write_json(identity, identity_payload)
        link = root / "preflight-link.json"
        write_json(
            link,
            {
                "schema": MODULE.PREFLIGHT_LINK_SCHEMA,
                "headless_run_report_hash": MODULE.sha256(run),
                "platform_run_report_hash": MODULE.sha256(identity),
                "build_fingerprint": run_payload["build_fingerprint"],
                "cooked_package_hash": run_payload["package_hash"],
                "input_sequence_hash": run_payload["input_sequence_hash"],
                "scenario": run_payload["scenario"],
                "target": run_payload["target"],
                "content_identity": run_payload["content_identity"],
                "headless_profile_id": run_payload["profile_id"],
                "headless_session_id": run_payload["session_id"],
                "platform_profile_id": identity_payload["profile_id"],
                "platform_session_id": identity_payload["session_id"],
            },
        )
        return {
            "package": package,
            "player": player,
            "run": run,
            "bundle": bundle,
            "review": review,
            "conformance": conformance,
            "identity": identity,
            "link": link,
        }

    def test_formal_review_and_platform_link_are_bound_to_the_same_run(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = self.fixture(pathlib.Path(temporary))
            run = MODULE.validate_headless_review(
                fixture["run"], fixture["bundle"], fixture["review"]
            )
            result = MODULE.validate_platform_preflight(
                "windows", run, fixture["run"], fixture["identity"], fixture["link"],
                fixture["player"], fixture["conformance"], MODULE.sha256(fixture["package"]),
            )
            self.assertEqual(result["status"], "pass")

    def test_review_failure_and_identity_drift_block_before_host_launch(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = self.fixture(pathlib.Path(temporary))
            review = MODULE.load(fixture["review"])
            review["checkpoints"][0]["passed"] = False
            write_json(fixture["review"], review)
            with self.assertRaisesRegex(RuntimeError, "formal Headless review blocked"):
                MODULE.validate_headless_review(
                    fixture["run"], fixture["bundle"], fixture["review"]
                )

            fixture = self.fixture(pathlib.Path(temporary))
            run = MODULE.validate_headless_review(
                fixture["run"], fixture["bundle"], fixture["review"]
            )
            identity = MODULE.load(fixture["identity"])
            identity["input_sequence_hash"] = "sha256:" + "9" * 64
            write_json(fixture["identity"], identity)
            with self.assertRaisesRegex(RuntimeError, "Headless preflight blocked"):
                MODULE.validate_platform_preflight(
                    "web", run, fixture["run"], fixture["identity"], fixture["link"],
                    fixture["player"], fixture["conformance"], MODULE.sha256(fixture["package"]),
                )


if __name__ == "__main__":
    unittest.main()
