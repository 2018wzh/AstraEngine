import hashlib
import json
import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image

from compare_classic_visual_references import (
    ComparisonError,
    LEGACY_DESKTOP_CAPTURE_REFERENCE_IDS,
    _capture_normalization,
    _entries_by_id,
    _mask,
    _json_hash,
    _ssim,
    _white_diamond_origins,
    compare,
)


def file_hash(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


class ClassicVisualComparisonTests(unittest.TestCase):
    def test_identical_region_has_exact_ssim(self):
        image = np.full((12, 12, 3), 96.0)
        self.assertEqual(_ssim(image, image), 1.0)

    def test_choice_geometry_tracks_diamond_origins_not_font_width(self):
        image = np.zeros((600, 800, 3), dtype=np.float64)
        for center_x, center_y in ((146, 132), (146, 172)):
            for y in range(center_y - 7, center_y + 8):
                for x in range(center_x - 7, center_x + 8):
                    if abs(x - center_x) + abs(y - center_y) <= 7:
                        image[y, x] = 255
        image[123:140, 210:222] = 255
        image[163:180, 228:242] = 255

        self.assertEqual(
            _white_diamond_origins(image, [100, 90, 250, 220], 2),
            [139, 125, 139, 165],
        )

    def test_choice_geometry_rejects_a_separate_focus_diamond(self):
        image = np.zeros((600, 800, 3), dtype=np.float64)
        for center_x, center_y in ((146, 132), (222, 132), (222, 172)):
            for y in range(center_y - 7, center_y + 8):
                for x in range(center_x - 7, center_x + 8):
                    if abs(x - center_x) + abs(y - center_y) <= 7:
                        image[y, x] = 255

        with self.assertRaisesRegex(ComparisonError, "CHOICE_DIAMOND_COUNT"):
            _white_diamond_origins(image, [100, 90, 250, 220], 2)

    def fixture(
        self,
        root: Path,
        *,
        capture: bool = True,
        capture_color: tuple[int, int, int] = (32, 48, 64),
    ):
        references, captures = root / "references", root / "captures"
        references.mkdir()
        captures.mkdir()
        reference = references / "tsui1999-ui-001-title.png"
        Image.new("RGB", (800, 600), (32, 48, 64)).save(reference)
        if capture:
            Image.new("RGB", (800, 600), capture_color).save(captures / "classic.fixture.png")
            Image.new("RGB", (800, 600), capture_color).save(captures / "classic.fixture.__stable.png")
        node_map = {
            "schema": "tsuinosora.classic_visual_node_map.v3",
            "entries": [{
                "reference_id": "TSUI1999-UI-001",
                "checkpoint": "classic.fixture",
                "comparison_class": "same_node",
                "identity": {
                    "movie_id": "FIXTURE",
                    "frame": 1,
                    "typed_state": "fixture.state",
                    "wait_command": "fixture.command",
                    "handler_id": "fixture.handler",
                    "locator": {
                        "method": "system_resource",
                        "content_sha256": "sha256:" + "1" * 64,
                    },
                    "reference_sha256": file_hash(reference),
                    "resource_hashes": ["sha256:" + "1" * 64],
                },
            }],
        }
        policy = {
            "schema": "tsuinosora.classic_visual_comparison_policy.v3",
            "thresholds": {"max_geometry_delta_px": 2, "min_ssim": 0.94, "max_perceptual_error": 0.08},
            "color_tolerance_profiles": {
                "capture_palette_v1": {
                    "reason_code": "capture_color_state_unproven",
                    "min_ssim": 0.75,
                    "max_perceptual_error": 0.12,
                }
            },
            "color_tolerance_approval": {
                "relative_path": "color-tolerance-approval.json",
                "sha256": "",
            },
            "capture_normalization": {
                "id": "windows_175pct_bilinear_then_lanczos_v1",
                "reference_ids": sorted(LEGACY_DESKTOP_CAPTURE_REFERENCE_IDS),
                "source_size": [800, 600],
                "captured_size": [1400, 1050],
                "upscale": "bilinear",
                "downscale": "lanczos",
            },
            "checks": [{
                "id": "classic.fixture",
                "reference_id": "TSUI1999-UI-001",
                "checkpoint": "classic.fixture",
                "mask": {"boxes": [], "max_coverage": 0.0},
                "geometry": [],
            }],
        }
        approval = {
            "schema": "astra.headless_tolerance_approval.v2",
            "approval_id": "tsui.classic.capture_palette_v1",
            "approver_kind": "human",
            "approver_identity": "project_owner",
            "approved_tolerance_hash": _json_hash(policy["color_tolerance_profiles"]),
            "previous_config_hash": None,
            "reason_codes": ["capture_color_state_unproven"],
        }
        approval_path = root / policy["color_tolerance_approval"]["relative_path"]
        approval_path.write_text(json.dumps(approval), encoding="utf-8")
        policy["color_tolerance_approval"]["sha256"] = file_hash(approval_path)
        checkpoint_identity = {"reference_id": "TSUI1999-UI-001", "typed_state": "fixture.state", "wait_command": "fixture.command"}
        acceptance = {
            "schema": "tsuinosora.classic_visual_acceptance_report.v2",
            "status": "passed",
            "text_locator_evidence_hash": _json_hash([{
                "reference_id": "TSUI1999-UI-001",
                "typed_state": "fixture.state",
                "wait_command": "fixture.command",
                "locator": node_map["entries"][0]["identity"]["locator"],
            }]),
            "runs": [{"checkpoint_nodes": {"classic.fixture": checkpoint_identity, "classic.fixture.__stable": checkpoint_identity}}],
        }
        return policy, node_map, acceptance, references, captures

    def test_same_node_generates_five_review_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root)
            report = compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)
            self.assertEqual(report["status"], "pass")
            artifact = root / "artifacts" / "classic.fixture"
            self.assertEqual(
                {path.name for path in artifact.iterdir()},
                {"reference.png", "capture.png", "mask.png", "absolute-diff.png", "perceptual-heatmap.png", "five-panel.png"},
            )

    def test_missing_capture_blocks_without_guessing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root, capture=False)
            report = compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)
        self.assertEqual(report["status"], "blocked")
        self.assertEqual(report["diagnostics"][0]["code"], "TSUI_CLASSIC_VISUAL_INPUT_MISSING")

    def test_missing_same_node_identity_is_reported_without_aborting_the_matrix(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root)
            acceptance["runs"][0]["checkpoint_nodes"].pop("classic.fixture")
            report = compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)
        self.assertEqual(report["status"], "blocked")
        self.assertEqual(report["check_count"], 0)
        self.assertEqual(
            report["diagnostics"],
            [{"code": "TSUI_CLASSIC_VISUAL_INPUT_EVIDENCE", "check_id": "classic.fixture", "capture": "primary"}],
        )

    def test_wrong_reference_hash_blocks(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root)
            node_map["entries"][0]["identity"]["reference_sha256"] = "sha256:" + "0" * 64
            report = compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)
        self.assertEqual(report["diagnostics"][0]["code"], "TSUI_CLASSIC_VISUAL_REFERENCE_HASH")

    def test_duplicate_checkpoint_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root)
            duplicate = dict(node_map["entries"][0])
            duplicate["reference_id"] = "TSUI1999-UI-002"
            node_map["entries"].append(duplicate)
            with self.assertRaisesRegex(ComparisonError, "NODE_MAP_UNIQUE"):
                compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)

    def test_same_node_requires_complete_identity_and_resource_closure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root)
            node_map["entries"][0]["identity"]["resource_hashes"] = []
            with self.assertRaisesRegex(ComparisonError, "RESOURCE_CLOSURE"):
                compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)

    def test_reference_retake_remains_blocking_even_when_pixels_match(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root)
            node_map["entries"][0]["reference_validation"] = {
                "status": "recapture_required",
                "reason_code": "source_presentation_contradiction",
                "required_evidence": "two_consecutive_frames",
            }
            report = compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)
        self.assertEqual(report["status"], "blocked")
        self.assertEqual(report["passed_count"], 0)
        self.assertEqual(
            report["diagnostics"][0]["code"],
            "TSUI_CLASSIC_VISUAL_REFERENCE_RECAPTURE_REQUIRED",
        )

    def test_opening_bitmap_accepts_exact_resource_closure_as_stability_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _, node_map, _, _, _ = self.fixture(root)
            entry = node_map["entries"][0]
            entry["reference_id"] = "TSUI1999-UI-002"
            resource_hash = "sha256:" + "1" * 64
            entry["identity"]["locator"] = {
                "method": "score_bitmap_text",
                "content_sha256": resource_hash,
            }
            entry["identity"]["resource_hashes"] = [resource_hash]
            entry["reference_validation"] = {
                "status": "verified",
                "method": "score_bitmap_resource_closure",
                "capture_sha256": entry["identity"]["reference_sha256"],
                "resource_sha256": resource_hash,
            }

            entries = _entries_by_id(node_map)

        self.assertIn("TSUI1999-UI-002", entries)

    def test_score_bitmap_stability_cannot_be_used_for_another_reference(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _, node_map, _, _, _ = self.fixture(root)
            entry = node_map["entries"][0]
            resource_hash = "sha256:" + "1" * 64
            entry["identity"]["locator"] = {
                "method": "score_bitmap_text",
                "content_sha256": resource_hash,
            }
            entry["reference_validation"] = {
                "status": "verified",
                "method": "score_bitmap_resource_closure",
                "capture_sha256": entry["identity"]["reference_sha256"],
                "resource_sha256": resource_hash,
            }

            with self.assertRaisesRegex(ComparisonError, "REFERENCE_VALIDATION"):
                _entries_by_id(node_map)

    def test_reference_retake_requires_bounded_reason_and_two_frames(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root)
            node_map["entries"][0]["reference_validation"] = {
                "status": "recapture_required",
                "reason_code": "unbounded-free-form-reason",
                "required_evidence": "one-frame",
            }
            with self.assertRaisesRegex(ComparisonError, "REFERENCE_VALIDATION"):
                compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)

    def test_explicit_color_tolerance_accepts_a_bounded_palette_shift(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(
                root, capture_color=(60, 76, 92)
            )
            node_map["entries"][0]["color_tolerance_approval"] = {
                "reason_code": "capture_color_state_unproven",
                "profile_id": "capture_palette_v1",
                "evidence": "same_node_resource_closure_and_stable_gpu_capture",
            }
            policy["checks"][0]["color_tolerance"] = "capture_palette_v1"
            report = compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)
        self.assertEqual(report["status"], "pass")
        self.assertEqual(report["passed_with_color_tolerance_count"], 1)
        self.assertTrue(report["results"][0]["color_tolerance"]["applied"])

    def test_color_tolerance_cannot_approve_a_source_presentation_contradiction(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root)
            node_map["entries"][0]["color_tolerance_approval"] = {
                "reason_code": "source_presentation_contradiction",
                "profile_id": "capture_palette_v1",
                "evidence": "same_node_resource_closure_and_stable_gpu_capture",
            }
            policy["checks"][0]["color_tolerance"] = "capture_palette_v1"
            with self.assertRaisesRegex(ComparisonError, "REFERENCE_VALIDATION"):
                compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)

    def test_color_tolerance_profile_cannot_be_relaxed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root)
            policy["color_tolerance_profiles"]["capture_palette_v1"][
                "max_perceptual_error"
            ] = 0.20
            with self.assertRaisesRegex(ComparisonError, "COLOR_TOLERANCE_POLICY"):
                compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)

    def test_color_tolerance_requires_node_approval(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root)
            policy["checks"][0]["color_tolerance"] = "capture_palette_v1"
            with self.assertRaisesRegex(ComparisonError, "COLOR_TOLERANCE_BINDING"):
                compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)

    def test_color_tolerance_requires_hash_bound_human_approval(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root)
            policy["color_tolerance_approval"]["sha256"] = "sha256:" + "0" * 64
            with self.assertRaisesRegex(ComparisonError, "approval hash mismatch"):
                compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)

    def test_excessive_mask_is_rejected(self):
        with self.assertRaisesRegex(ComparisonError, "MASK_EXCESSIVE"):
            _mask((600, 800), [[0, 0, 800, 300]], 0.20)

    def test_thresholds_cannot_be_relaxed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root)
            policy["thresholds"]["min_ssim"] = 0.90
            with self.assertRaisesRegex(ComparisonError, "FIXED_THRESHOLDS"):
                compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)

    def test_capture_normalization_cannot_be_changed_to_raise_similarity(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, node_map, acceptance, references, captures = self.fixture(root)
            policy["capture_normalization"]["upscale"] = "bicubic"
            with self.assertRaisesRegex(ComparisonError, "CAPTURE_NORMALIZATION"):
                compare(policy, node_map, acceptance, references, captures, root / "artifacts", root)

    def test_native_client_recaptures_are_not_resampled(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            policy, _, _, _, _ = self.fixture(root)
            normalized, normalization_id = _capture_normalization(policy)
            self.assertEqual(normalization_id, "windows_175pct_bilinear_then_lanczos_v1")
            self.assertNotIn("TSUI1999-UI-003", normalized)
            self.assertNotIn("TSUI1999-UI-005", normalized)
            self.assertNotIn("TSUI1999-UI-009", normalized)


if __name__ == "__main__":
    unittest.main()
