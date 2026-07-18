import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from headless_gpu_acceptance import (
    GpuAcceptanceError,
    file_hash,
    prepare_gpu_profile,
    validate_gpu_artifacts,
)


class HeadlessGpuAcceptanceTests(unittest.TestCase):
    def test_prepare_gpu_profile_rebinds_only_supported_renderer(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.json"
            output = root / "gpu.json"
            source.write_text(
                json.dumps(
                    {
                        "schema": "astra.headless_host_profile.v2",
                        "id": "gpu.fixture",
                        "providers": {"renderer": "cpu_reference", "text": "cosmic_text_cpu"},
                    }
                ),
                encoding="utf-8",
            )
            profile = prepare_gpu_profile(source, output)
            self.assertEqual(profile["providers"]["renderer"], "wgpu_offscreen")
            self.assertEqual(profile["providers"]["text"], "cosmic_text_cpu")
            self.assertEqual(json.loads(output.read_text())["providers"]["renderer"], "wgpu_offscreen")

    def test_prepare_gpu_profile_rejects_unknown_renderer(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.json"
            source.write_text(json.dumps({"providers": {"renderer": "implicit"}}), encoding="utf-8")
            with self.assertRaisesRegex(GpuAcceptanceError, "cannot be promoted"):
                prepare_gpu_profile(source, root / "gpu.json")

    @patch("headless_gpu_acceptance.expected_backend", return_value="dx12")
    def test_validate_gpu_artifacts_requires_matching_hardware_identity(self, _backend):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest_path = root / "artifact-manifest.json"
            manifest = {
                "schema": "astra.headless_artifact_manifest.v2",
                "build_fingerprint": "sha256:build",
                "package_hash": "sha256:package",
                "renderer_identity_hash": "sha256:renderer",
                "renderer_identity": {
                    "provider": "wgpu_offscreen",
                    "backend": "dx12",
                    "device_type": "discrete_gpu",
                },
                "submitted_frame_count": 9,
                "rasterized_frame_count": 3,
            }
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            report = {
                "schema": "astra.headless_run_report.v2",
                "status": "passed",
                "build_fingerprint": "sha256:build",
                "package_hash": "sha256:package",
                "completed_sequence": 7,
                "manifest_hash": file_hash(manifest_path),
                "renderer_identity_hash": "sha256:renderer",
                "checkpoint_results": [{"id": "classic.title", "passed": True}],
            }
            (root / "run-report.json").write_text(json.dumps(report), encoding="utf-8")
            validate_gpu_artifacts(
                root,
                build_fingerprint="sha256:build",
                package_hash="sha256:package",
                completed_sequence=7,
                checkpoint_ids=["classic.title"],
            )
            manifest["renderer_identity"]["device_type"] = "cpu"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            report["manifest_hash"] = file_hash(manifest_path)
            (root / "run-report.json").write_text(json.dumps(report), encoding="utf-8")
            with self.assertRaisesRegex(GpuAcceptanceError, "non-hardware"):
                validate_gpu_artifacts(
                    root,
                    build_fingerprint="sha256:build",
                    package_hash="sha256:package",
                    completed_sequence=7,
                    checkpoint_ids=["classic.title"],
                )


if __name__ == "__main__":
    unittest.main()
