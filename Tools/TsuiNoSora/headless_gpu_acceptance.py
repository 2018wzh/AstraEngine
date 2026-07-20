#!/usr/bin/env python3
"""Strict GPU profile preparation and artifact validation for TsuiNoSora E2."""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path


RUN_REPORT_SCHEMA = "astra.headless_run_report.v2"
ARTIFACT_MANIFEST_SCHEMA = "astra.headless_artifact_manifest.v2"
GPU_PROVIDER = "wgpu_offscreen"
HARDWARE_DEVICE_TYPES = {"discrete_gpu", "integrated_gpu"}


class GpuAcceptanceError(RuntimeError):
    pass


def file_hash(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def expected_backend() -> str:
    if sys.platform == "win32":
        return "dx12"
    if sys.platform.startswith("linux"):
        return "vulkan"
    if sys.platform == "darwin":
        return "metal"
    raise GpuAcceptanceError("Headless GPU acceptance is unsupported on this host platform")


def prepare_gpu_profile(source: Path, output: Path) -> dict:
    profile = json.loads(source.read_text(encoding="utf-8"))
    providers = profile.get("providers")
    if not isinstance(providers, dict) or not isinstance(providers.get("renderer"), str):
        raise GpuAcceptanceError("Headless profile has no explicit renderer binding")
    if providers["renderer"] not in {"cpu_reference", GPU_PROVIDER}:
        raise GpuAcceptanceError("Headless profile renderer cannot be promoted to wgpu_offscreen")
    profile = json.loads(json.dumps(profile))
    profile["providers"]["renderer"] = GPU_PROVIDER
    output.write_text(
        json.dumps(profile, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return profile


def validate_gpu_artifacts(
    artifact_root: Path,
    *,
    build_fingerprint: str,
    package_hash: str,
    completed_sequence: int,
    checkpoint_ids: list[str],
) -> tuple[dict, dict]:
    run_report_path = artifact_root / "run-report.json"
    manifest_path = artifact_root / "artifact-manifest.json"
    run_report = json.loads(run_report_path.read_text(encoding="utf-8"))
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if run_report.get("schema") != RUN_REPORT_SCHEMA or run_report.get("status") != "passed":
        raise GpuAcceptanceError("Headless GPU run did not produce a passed v2 run report")
    if manifest.get("schema") != ARTIFACT_MANIFEST_SCHEMA:
        raise GpuAcceptanceError("Headless GPU run did not produce a v2 artifact manifest")
    if run_report.get("completed_sequence") != completed_sequence:
        raise GpuAcceptanceError("Headless GPU run did not consume every physical input")
    if (
        run_report.get("build_fingerprint") != build_fingerprint
        or manifest.get("build_fingerprint") != build_fingerprint
        or run_report.get("package_hash") != package_hash
        or manifest.get("package_hash") != package_hash
    ):
        raise GpuAcceptanceError("Headless GPU run identity diverged from the prepared profile")
    if run_report.get("manifest_hash") != file_hash(manifest_path):
        raise GpuAcceptanceError("Headless GPU run report and artifact manifest hash diverged")
    renderer = manifest.get("renderer_identity")
    if not isinstance(renderer, dict):
        raise GpuAcceptanceError("Headless GPU artifact manifest has no renderer identity")
    if renderer.get("provider") != GPU_PROVIDER:
        raise GpuAcceptanceError("Headless visual evidence used a non-GPU renderer provider")
    if renderer.get("backend") != expected_backend():
        raise GpuAcceptanceError("Headless GPU backend does not match the native host contract")
    if renderer.get("device_type") not in HARDWARE_DEVICE_TYPES:
        raise GpuAcceptanceError("Headless GPU evidence used a non-hardware adapter")
    if run_report.get("renderer_identity_hash") != manifest.get("renderer_identity_hash"):
        raise GpuAcceptanceError("Headless GPU renderer identity hash diverged")
    checkpoints = run_report.get("checkpoint_results")
    if not isinstance(checkpoints, list) or [item.get("id") for item in checkpoints] != checkpoint_ids:
        raise GpuAcceptanceError("Headless GPU run has an invalid checkpoint set")
    if not all(item.get("passed") is True for item in checkpoints):
        raise GpuAcceptanceError("Headless GPU checkpoint comparison failed")
    if manifest.get("rasterized_frame_count", 0) < len(checkpoint_ids):
        raise GpuAcceptanceError("Headless GPU run did not rasterize every required checkpoint")
    if manifest.get("submitted_frame_count", 0) < manifest.get("rasterized_frame_count", 0):
        raise GpuAcceptanceError("Headless GPU frame counters are inconsistent")
    return run_report, manifest
