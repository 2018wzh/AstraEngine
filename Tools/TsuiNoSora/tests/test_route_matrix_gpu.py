import argparse
import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from classic_visual_acceptance import InputBudgetError
from headless_gpu_acceptance import GpuAcceptanceError, file_hash
from headless_route_matrix import RouteContract, _run_route, run_matrix


class MatrixGpuTests(unittest.TestCase):
    def test_runner_passes_gpu_flag_and_rejects_software_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            profile = root / "profile.json"
            profile.write_text(json.dumps(dict(build_fingerprint="build", package_hash="package")))
            contract = RouteContract("route.coverage.001", "end", "state.end", (), (),
                                     root / "input.jsonl", 7, "input")

            def child(command, **kwargs):
                self.assertIn("--gpu", command)
                directory = Path(command[command.index("--artifact-root") + 1])
                manifest = dict(schema="astra.headless_artifact_manifest.v2",
                                build_fingerprint="build", package_hash="package",
                                renderer_identity_hash="renderer",
                                renderer_identity=dict(provider="wgpu_offscreen", backend="dx12",
                                                       device_type=device_type),
                                submitted_frame_count=9, rasterized_frame_count=3)
                manifest_path = directory / "artifact-manifest.json"
                manifest_path.write_text(json.dumps(manifest))
                report = dict(schema="astra.headless_run_report.v2", status="passed",
                              session_id="tsui.route.coverage.001", build_fingerprint="build",
                              package_hash="package", completed_sequence=7,
                              input_sequence_hash="input", renderer_identity_hash="renderer",
                              manifest_hash=file_hash(manifest_path), diagnostics=[],
                              submitted_frame_count=9, rasterized_frame_count=3,
                              audio_frame_count=42, duration_ns=1,
                              checkpoint_results=[dict(id="checkpoint.route.coverage.001",
                                                       passed=True, observation_hash="checkpoint")])
                (directory / "run-report.json").write_text(json.dumps(report))
                return SimpleNamespace(returncode=0)

            with patch("headless_route_matrix.subprocess.run", side_effect=child), \
                 patch("headless_gpu_acceptance.expected_backend", return_value="dx12"):
                for device_type in ["discrete_gpu", "cpu"]:
                    arguments = dict(binary=root / "binary", profile=profile, package=root / "package",
                                     build_identity=root / "identity", artifact_root=root / device_type,
                                     timeout_seconds=30)
                    if device_type == "cpu":
                        with self.assertRaisesRegex(GpuAcceptanceError, "non-hardware"):
                            _run_route(contract, **arguments)
                    else:
                        result = _run_route(contract, **arguments)
                        self.assertEqual(result["submitted_frame_count"], 9)
                        self.assertEqual(result["renderer_identity_hash"], "renderer")

    def test_input_budget_failure_precedes_process_and_output_creation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in ["binary", "package"]:
                (root / name).write_bytes(b"fixture")
            profile = dict(build_fingerprint="build", package_hash=file_hash(root / "package"),
                           providers=dict(renderer="wgpu_offscreen"),
                           input=dict(max_messages=1, max_tick=1))
            (root / "profile").write_text(json.dumps(profile))
            (root / "identity").write_text(json.dumps(dict(schema="astra.build_identity.v1", identity_hash="build")))
            (root / "story").write_text(json.dumps(dict(schema="tsuinosora.native_story_ir.v1",
                                                       routes=[dict(route_id="route.coverage.001")])))
            inputs = root / "inputs"
            inputs.mkdir()
            (inputs / "route.coverage.001.jsonl").write_text('{"tick":0}\n{"tick":2}\n')
            args = argparse.Namespace(binary=root / "binary", profile=root / "profile",
                                      package=root / "package", build_identity=root / "identity",
                                      automation_root=inputs, native_story_ir=root / "story",
                                      artifact_root=root / "output", jobs=1, timeout_seconds=30)
            with patch("headless_route_matrix._validate_route_input"), \
                 patch("headless_route_matrix.subprocess.run") as child:
                with self.assertRaises(InputBudgetError):
                    run_matrix(args)
                child.assert_not_called()
            self.assertFalse(args.artifact_root.exists())


if __name__ == "__main__":
    unittest.main()
