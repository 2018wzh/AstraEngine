import json
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path

from private_rc_release_gate import REPORT_SCHEMA, build_report, sha256_file


class PrivateRcReleaseGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.bundle = self.root / "bundle"
        (self.bundle / "package").mkdir(parents=True)
        (self.bundle / "source").mkdir()
        self.package = self.root / "game.astrapkg"
        self.package.write_bytes(b"encrypted-commercial-payload")
        (self.bundle / "package" / "nativevn.astrapkg").write_bytes(self.package.read_bytes())
        self.source = self.root / "source.json"
        self.write_json(
            self.source,
            {
                "schema": "astra.source_verification_manifest.v1",
                "profile_id": "verified",
                "manifest_hash": "sha256:" + "1" * 64,
                "entries": [{"relative_path": "DATA", "byte_length": 1, "sha256": "sha256:" + "2" * 64}],
            },
        )
        (self.bundle / "source" / "source-profile.json").write_bytes(self.source.read_bytes())
        package_hash = sha256_file(self.package)
        source_hash = sha256_file(self.source)
        self.write_json(
            self.bundle / "bundle_manifest.json",
            {
                "schema": "astra.standalone_bundle_manifest.v2",
                "target": "tsuinosora-internal-game",
                "profile": "classic",
                "platform": "windows",
                "entrypoint": "AstraPlayer.exe",
                "package_hash": package_hash,
                "package": "package/nativevn.astrapkg",
                "files": [
                    self.file_entry("package/nativevn.astrapkg", "package"),
                    self.file_entry("source/source-profile.json", "source_verification_profile"),
                ],
            },
        )
        self.build = self.root / "build.json"
        build_hash = "sha256:" + "3" * 64
        self.write_json(self.build, {"schema": "astra.build_identity.v1", "identity_hash": build_hash})
        self.node_map = self.root / "node-map.json"
        entries = []
        for index in range(1, 16):
            reference_id = f"TSUI1999-UI-{index:03d}"
            reference_hash = "sha256:" + f"{index:064x}"
            identity = {
                "movie_id": (
                    "Y"
                    if reference_id
                    in {"TSUI1999-UI-003", "TSUI1999-UI-005", "TSUI1999-UI-009"}
                    else "K"
                ),
                "reference_sha256": reference_hash,
            }
            if reference_id == "TSUI1999-UI-002":
                resource_hash = "sha256:" + "f" * 64
                identity.update(
                    {
                        "locator": {
                            "method": "score_bitmap_text",
                            "content_sha256": resource_hash,
                        },
                        "resource_hashes": [resource_hash],
                    }
                )
                validation = {
                    "status": "verified",
                    "method": "score_bitmap_resource_closure",
                    "capture_sha256": reference_hash,
                    "resource_sha256": resource_hash,
                }
            else:
                validation = {
                    "status": "verified",
                    "method": "byte_identical_stable_pair",
                    "capture_pair_sha256": reference_hash,
                }
            entries.append(
                {
                    "reference_id": reference_id,
                    "checkpoint": f"checkpoint.{index}",
                    "identity": identity,
                    "reference_validation": validation,
                }
            )
        self.write_json(self.node_map, {"schema": "tsuinosora.classic_visual_node_map.v3", "entries": entries})
        self.headless = self.root / "headless.json"
        checkpoints = [entry["checkpoint"] for entry in entries]
        renderer = {"provider": "wgpu_offscreen", "backend": "dx12", "device_type": "discrete_gpu"}
        self.write_json(
            self.headless,
            {
                "schema": "tsuinosora.classic_visual_acceptance_report.v2",
                "status": "passed",
                "build_fingerprint": build_hash,
                "package_hash": package_hash,
                "checkpoint_count": len(checkpoints),
                "checkpoint_ids": checkpoints,
                "renderer_identity_hash": "sha256:" + "4" * 64,
                "diagnostics": [],
                "runs": [{"renderer_identity": renderer, "renderer_identity_hash": "sha256:" + "4" * 64}],
            },
        )
        self.comparison = self.root / "comparison.json"
        self.write_json(
            self.comparison,
            {
                "schema": "tsuinosora.classic_visual_comparison_report.v3",
                "status": "passed",
                "check_count": 15,
                "passed_count": 15,
                "results": [{"reference_id": entry["reference_id"], "status": "pass"} for entry in entries],
                "diagnostics": [],
            },
        )
        self.coverage = self.root / "coverage.json"
        self.y_route = self.root / "y-route.json"
        self.write_json(
            self.y_route,
            {
                "schema": "tsuinosora.classic_y_route_acceptance_report.v1",
                "status": "passed",
                "route_id": "route.coverage.001",
                "guaranteed_movie": "Y",
                "boundary_movie": "K",
                "boundary_state": "director.k.0010.score.0010",
                "boundary_wait_command": "tsui.command.000060",
                "checkpoint_id": "classic.route.y.complete",
                "choice_selection_count": 3,
                "choice_sequence_hash": "sha256:" + "5" * 64,
                "build_fingerprint": build_hash,
                "package_hash": package_hash,
                "renderer_identity": renderer,
                "diagnostics": [],
            },
        )
        self.write_json(
            self.coverage,
            {"schema": "tsuinosora.full_conversion_coverage_report.v1", "status": "pass", "counts": {"routes": 37}, "diagnostics": []},
        )
        self.story = self.root / "story.json"
        self.write_json(
            self.story,
            {
                "schema": "tsuinosora.native_story_ir.v1",
                "text": "a sufficiently long commercial line",
                "stories": [
                    {
                        "states": [
                            {
                                "state_id": "director.k.0010.score.0010",
                                "scenes": [
                                    {
                                        "commands": [
                                            {
                                                "command_id": "tsui.command.000060",
                                                "kind": "timeline",
                                                "duration_ms": 9000,
                                            }
                                        ]
                                    }
                                ],
                            }
                        ]
                    }
                ],
            },
        )
        self.paths = self.root / "paths.json"
        self.write_json(self.paths, ["X:/private/source", "X:/private/workspace"])
        self.signoff = self.root / "signoff.json"
        self.write_json(
            self.signoff,
            {
                "schema": "tsuinosora.private_rc_manual_signoff.v1",
                "status": "approved",
                "reviewer": "reviewer",
                "build_identity": build_hash,
                "package_identity": package_hash,
                "headless_report_sha256": sha256_file(self.headless),
                "comparison_report_sha256": sha256_file(self.comparison),
                "y_route_report_sha256": sha256_file(self.y_route),
            },
        )

    def tearDown(self) -> None:
        self.temp.cleanup()

    def write_json(self, path: Path, value: object) -> None:
        path.write_text(json.dumps(value), encoding="utf-8")

    def file_entry(self, relative: str, role: str) -> dict[str, object]:
        path = self.bundle / Path(relative)
        return {"path": relative, "role": role, "hash": sha256_file(path), "byte_size": path.stat().st_size}

    def args(self) -> Namespace:
        return Namespace(
            package=self.package,
            bundle=self.bundle,
            build_identity=self.build,
            headless_report=self.headless,
            y_route_report=self.y_route,
            comparison_report=self.comparison,
            node_map=self.node_map,
            coverage_report=self.coverage,
            source_profile=self.source,
            story_ir=self.story,
            private_path_probes=self.paths,
            manual_signoff=self.signoff,
        )

    def test_complete_evidence_passes(self) -> None:
        report = build_report(self.args())
        self.assertEqual(report["schema"], REPORT_SCHEMA)
        self.assertEqual(report["status"], "passed")
        self.assertEqual(report["scope"]["guaranteed_routes"], ["Y"])
        self.assertEqual(report["scope"]["present_unvalidated_route_count"], 36)

    def test_missing_visuals_and_signoff_remain_blocking(self) -> None:
        comparison = json.loads(self.comparison.read_text(encoding="utf-8"))
        comparison["status"] = "blocked"
        comparison["check_count"] = 3
        comparison["passed_count"] = 3
        comparison["results"] = comparison["results"][:3]
        self.write_json(self.comparison, comparison)
        args = self.args()
        args.manual_signoff = None
        report = build_report(args)
        self.assertEqual(report["status"], "blocked")
        blocked = {check["id"] for check in report["checks"] if check["status"] == "blocking"}
        self.assertEqual(blocked, {"visual_reference_y_13_of_13", "formal_human_signoff"})

    def test_missing_stable_reference_evidence_is_blocking(self) -> None:
        node_map = json.loads(self.node_map.read_text(encoding="utf-8"))
        target = next(
            entry
            for entry in node_map["entries"]
            if entry["reference_id"] == "TSUI1999-UI-013"
        )
        target.pop("reference_validation")
        self.write_json(self.node_map, node_map)

        report = build_report(self.args())

        blocked = {check["id"] for check in report["checks"] if check["status"] == "blocking"}
        self.assertIn("visual_reference_y_13_of_13", blocked)

    def test_unvalidated_k_references_do_not_block_y_scoped_rc(self) -> None:
        comparison = json.loads(self.comparison.read_text(encoding="utf-8"))
        comparison["status"] = "blocked"
        for result in comparison["results"]:
            if result["reference_id"] in {"TSUI1999-UI-004", "TSUI1999-UI-015"}:
                result["status"] = "blocked"
        comparison["passed_count"] = 13
        comparison["diagnostics"] = [
            {"code": "UNVALIDATED_ROUTE", "check_id": "checkpoint.4"},
            {"code": "UNVALIDATED_ROUTE", "check_id": "checkpoint.15"},
        ]
        self.write_json(self.comparison, comparison)
        self.write_json(
            self.signoff,
            {
                "schema": "tsuinosora.private_rc_manual_signoff.v1",
                "status": "approved",
                "reviewer": "reviewer",
                "build_identity": "sha256:" + "3" * 64,
                "package_identity": sha256_file(self.package),
                "headless_report_sha256": sha256_file(self.headless),
                "comparison_report_sha256": sha256_file(self.comparison),
                "y_route_report_sha256": sha256_file(self.y_route),
            },
        )
        report = build_report(self.args())
        self.assertEqual(report["status"], "passed")
        self.assertEqual(report["counts"]["visual_checks_required"], 13)

    def test_plaintext_or_path_leak_blocks(self) -> None:
        self.package.write_bytes(self.package.read_bytes() + b"a sufficiently long commercial line")
        (self.bundle / "package" / "nativevn.astrapkg").write_bytes(self.package.read_bytes())
        manifest = json.loads((self.bundle / "bundle_manifest.json").read_text(encoding="utf-8"))
        manifest["package_hash"] = sha256_file(self.package)
        manifest["files"][0] = self.file_entry("package/nativevn.astrapkg", "package")
        self.write_json(self.bundle / "bundle_manifest.json", manifest)
        report = build_report(self.args())
        blocked = {check["id"] for check in report["checks"] if check["status"] == "blocking"}
        self.assertIn("commercial_plaintext_absent", blocked)

    def test_y_boundary_command_is_derived_from_story_ir(self) -> None:
        story = json.loads(self.story.read_text(encoding="utf-8"))
        story["stories"][0]["states"][0]["scenes"][0]["commands"][0]["command_id"] = (
            "tsui.command.009999"
        )
        self.write_json(self.story, story)
        report = build_report(self.args())
        blocked = {check["id"] for check in report["checks"] if check["status"] == "blocking"}
        self.assertIn("guaranteed_route_y", blocked)


if __name__ == "__main__":
    unittest.main()
