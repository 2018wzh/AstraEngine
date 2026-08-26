from __future__ import annotations

import hashlib
import io
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import zlib
from collections import deque
from pathlib import Path

from tsuinosora_constants import *
from tsuinosora_visual_image_utils import _normalize_visual_capture_image, _visual_nonblank_bbox, _rgba_crop_bytes, _resize_rgba_bytes
from tsuinosora_diagnostics import _dedupe_diagnostics, _is_safe_report_relative_path, _is_safe_symbol, _write_json
from tsuinosora_rendering import _float_threshold, _non_negative_int, _report_has_path_leak, _safe_work_relative_path, _is_sha256
from tsuinosora_visual_analysis import _read_png_rgba, _rgba_delta_metrics, _rgba_nonblank, _rgba_region

__all__ = ['build_visual_comparison_report', '_read_visual_comparison_image', '_visual_capture_checkpoint', '_visual_capture_image', '_visual_region_record', '_visual_reviews_by_checkpoint', '_visual_review_record', '_compare_visual_region']


def build_visual_comparison_report(work_root: Path | str, capture_report: dict, visual_reviews: list[dict]) -> dict:
    work_root = Path(work_root)
    diagnostics = []
    checkpoints = []
    if not isinstance(capture_report, dict):
        capture_report = {}
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_COMPARISON_CAPTURE_REPORT_INVALID",
                "message": "visual comparison requires a screenshot capture report",
            }
        )
    thresholds = capture_report.get("thresholds", {}) if isinstance(capture_report, dict) else {}
    max_mean_delta = _float_threshold(thresholds, "max_mean_delta", 4.0)
    max_changed_ratio = _float_threshold(thresholds, "max_changed_ratio", 0.05)
    review_by_checkpoint = _visual_reviews_by_checkpoint(visual_reviews, diagnostics)
    if capture_report.get("schema") != "tsuinosora.visual_screenshot_capture_report.v1":
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_COMPARISON_CAPTURE_REPORT_INVALID",
                "message": "visual comparison requires a screenshot capture report",
            }
        )
    if capture_report.get("status") != "pass":
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_COMPARISON_CAPTURE_BLOCKED",
                "message": "visual comparison requires passing screenshot capture evidence",
            }
        )
        diagnostics.extend(capture_report.get("diagnostics", []))
    for checkpoint in capture_report.get("checkpoints", []):
        checkpoint_id = str(checkpoint.get("checkpoint_id", "unknown"))
        route_id = str(checkpoint.get("route_id", "unknown"))
        required = bool(checkpoint.get("required", True))
        review = review_by_checkpoint.get(checkpoint_id)
        checkpoint_record = {
            "checkpoint_id": checkpoint_id,
            "route_id": route_id,
            "required": required,
            "regions": [],
            "visual_review": _visual_review_record(review),
        }
        if required and not review:
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_COMPARISON_REVIEW_MISSING",
                    "checkpoint_id": checkpoint_id,
                    "message": "required visual checkpoint is missing vision review evidence",
                }
            )
        elif required and review.get("status") != "pass":
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_COMPARISON_REVIEW_BLOCKED",
                    "checkpoint_id": checkpoint_id,
                    "message": "vision review did not pass for a required checkpoint",
                }
            )
        original_path = _safe_work_relative_path(checkpoint.get("original", {}).get("path", ""))
        demo_path = _safe_work_relative_path(checkpoint.get("demo", {}).get("path", ""))
        original_image = _read_visual_comparison_image(work_root, original_path, checkpoint_id, "original", diagnostics)
        demo_image = _read_visual_comparison_image(work_root, demo_path, checkpoint_id, "demo", diagnostics)
        if original_image and demo_image and original_image["dimensions"] != demo_image["dimensions"]:
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_COMPARISON_DIMENSION_MISMATCH",
                    "checkpoint_id": checkpoint_id,
                    "message": "original and demo screenshots have different dimensions",
                }
            )
        if original_image and demo_image:
            for region in checkpoint.get("regions", []):
                region_record, region_diagnostics = _compare_visual_region(
                    checkpoint_id,
                    original_image,
                    demo_image,
                    region,
                    max_mean_delta,
                    max_changed_ratio,
                )
                checkpoint_record["regions"].append(region_record)
                diagnostics.extend(region_diagnostics)
        checkpoints.append(checkpoint_record)
    report = {
        "schema": "tsuinosora.visual_comparison_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "thresholds": {
            "max_mean_delta": max_mean_delta,
            "max_changed_ratio": max_changed_ratio,
        },
        "checkpoints": checkpoints,
        "diagnostics": _dedupe_diagnostics(diagnostics),
        "redaction": {
            "paths": "work_root_relative_only",
            "payload": "omitted",
            "commercial_text": "omitted",
            "screenshots": "omitted",
            "audio": "omitted",
            "movie": "omitted",
        },
    }
    if _report_has_path_leak(report):
        report["status"] = "blocked"
        report["diagnostics"].append(
            {
                "code": "TSUI_VISUAL_COMPARISON_REPORT_PATH_LEAK",
                "message": "visual comparison report contains a local path-like value",
            }
        )
        report["diagnostics"] = _dedupe_diagnostics(report["diagnostics"])
    _write_json(work_root / "reports" / "visual_comparison_report.json", report)
    return report


def _read_visual_comparison_image(
    work_root: Path,
    relative_path: str,
    checkpoint_id: str,
    role: str,
    diagnostics: list[dict],
) -> dict | None:
    if not relative_path:
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_COMPARISON_SCREENSHOT_PATH_INVALID",
                "checkpoint_id": checkpoint_id,
                "role": role,
                "message": "visual comparison screenshot path must be work-root relative",
            }
        )
        return None
    path = work_root / relative_path
    if not path.is_file():
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_COMPARISON_SCREENSHOT_MISSING",
                "checkpoint_id": checkpoint_id,
                "role": role,
                "message": "visual comparison screenshot is missing",
            }
        )
        return None
    try:
        return _read_png_rgba(path)
    except (OSError, ValueError, zlib.error, struct.error):
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_COMPARISON_SCREENSHOT_INVALID",
                "checkpoint_id": checkpoint_id,
                "role": role,
                "message": "visual comparison screenshot must be a readable PNG",
            }
        )
        return None


def _visual_capture_checkpoint(work_root: Path, raw: dict) -> tuple[dict, list[dict]]:
    diagnostics = []
    if not isinstance(raw, dict):
        raw = {}
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_CHECKPOINT_INVALID",
                "message": "visual checkpoint must be an object",
            }
        )
    checkpoint_id = str(raw.get("checkpoint_id", "unknown"))
    route_id = str(raw.get("route_id", "unknown"))
    required = bool(raw.get("required", True))
    if not _is_safe_symbol(checkpoint_id):
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_CHECKPOINT_ID_INVALID",
                "checkpoint_id": checkpoint_id or "unknown",
                "message": "visual checkpoint id must be a safe symbol",
            }
        )
        checkpoint_id = "unknown"
    if not _is_safe_symbol(route_id):
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_ROUTE_ID_INVALID",
                "checkpoint_id": checkpoint_id,
                "message": "visual checkpoint route id must be a safe symbol",
            }
        )
        route_id = "unknown"
    original = _visual_capture_image(work_root, raw.get("original_screenshot", ""), checkpoint_id, "original", diagnostics)
    demo = _visual_capture_image(work_root, raw.get("demo_screenshot", ""), checkpoint_id, "demo", diagnostics)
    regions = []
    raw_regions = raw.get("regions", [])
    if not isinstance(raw_regions, list) or not raw_regions:
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_REGIONS_MISSING",
                "checkpoint_id": checkpoint_id,
                "message": "visual checkpoint requires at least one region",
            }
        )
        raw_regions = []
    for region in raw_regions:
        regions.append(_visual_region_record(region, checkpoint_id, diagnostics, original.get("dimensions", {})))
    return (
        {
            "checkpoint_id": checkpoint_id,
            "route_id": route_id,
            "required": required,
            "original": original,
            "demo": demo,
            "regions": regions,
        },
        diagnostics,
    )


def _visual_capture_image(work_root: Path, value: object, checkpoint_id: str, role: str, diagnostics: list[dict]) -> dict:
    rel = str(value).strip() if isinstance(value, str) else ""
    entry = {
        "path": rel if _is_safe_report_relative_path(rel) else "",
        "hash": "",
        "dimensions": {"width": 0, "height": 0},
        "nonblank": False,
    }
    if not rel or not _is_safe_report_relative_path(rel):
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_PATH_INVALID",
                "checkpoint_id": checkpoint_id,
                "role": role,
                "message": "visual screenshot path must be work-root relative",
            }
        )
        return entry
    path = work_root / rel
    if not path.is_file():
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_SCREENSHOT_MISSING",
                "checkpoint_id": checkpoint_id,
                "role": role,
                "path": rel,
                "message": "visual screenshot file is missing",
            }
        )
        return entry
    try:
        image = _read_png_rgba(path)
    except (OSError, ValueError, zlib.error, struct.error):
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_PNG_INVALID",
                "checkpoint_id": checkpoint_id,
                "role": role,
                "path": rel,
                "message": "visual screenshot must be a readable PNG",
            }
        )
        return entry
    entry["hash"] = _sha256(path)
    entry["dimensions"] = image["dimensions"]
    entry["nonblank"] = _rgba_nonblank(image["pixels"])
    if not entry["nonblank"]:
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_BLANK",
                "checkpoint_id": checkpoint_id,
                "role": role,
                "path": rel,
                "message": "visual screenshot is blank or fully transparent",
            }
        )
    return entry


def _visual_region_record(region: object, checkpoint_id: str, diagnostics: list[dict], dimensions: dict) -> dict:
    if not isinstance(region, dict):
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_REGION_INVALID",
                "checkpoint_id": checkpoint_id,
                "message": "visual region must be an object",
            }
        )
        region = {}
    region_id = str(region.get("region_id", "unknown"))
    if not _is_safe_symbol(region_id):
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_REGION_ID_INVALID",
                "checkpoint_id": checkpoint_id,
                "message": "visual region id must be a safe symbol",
            }
        )
        region_id = "unknown"
    x = _non_negative_int(region.get("x", 0))
    y = _non_negative_int(region.get("y", 0))
    width = _non_negative_int(region.get("width", 0))
    height = _non_negative_int(region.get("height", 0))
    if width == 0:
        width = max(_non_negative_int(dimensions.get("width", 0)) - x, 0)
    if height == 0:
        height = max(_non_negative_int(dimensions.get("height", 0)) - y, 0)
    if width == 0 or height == 0:
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_CAPTURE_REGION_EMPTY",
                "checkpoint_id": checkpoint_id,
                "region_id": region_id,
                "message": "visual region must have positive dimensions",
            }
        )
    return {
        "region_id": region_id,
        "x": x,
        "y": y,
        "width": width,
        "height": height,
        "required": bool(region.get("required", True)),
    }


def _visual_reviews_by_checkpoint(visual_reviews: list[dict], diagnostics: list[dict]) -> dict[str, dict]:
    reviews = {}
    if not isinstance(visual_reviews, list):
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_COMPARISON_REVIEW_INVALID",
                "message": "visual_reviews must be a list",
            }
        )
        return reviews
    for raw in visual_reviews:
        if not isinstance(raw, dict):
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_COMPARISON_REVIEW_INVALID",
                    "message": "visual review entries must be objects",
                }
            )
            continue
        checkpoint_id = str(raw.get("checkpoint_id", ""))
        status = str(raw.get("status", ""))
        reviewer = str(raw.get("reviewer", ""))
        summary_hash = str(raw.get("summary_hash", ""))
        if not _is_safe_symbol(checkpoint_id) or status not in {"pass", "blocked"} or not _is_safe_symbol(reviewer):
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_COMPARISON_REVIEW_INVALID",
                    "checkpoint_id": checkpoint_id or "unknown",
                    "message": "visual review must use safe checkpoint id, reviewer and status",
                }
            )
            continue
        if not _is_sha256(summary_hash):
            diagnostics.append(
                {
                    "code": "TSUI_VISUAL_COMPARISON_REVIEW_HASH_INVALID",
                    "checkpoint_id": checkpoint_id,
                    "message": "visual review summary must be represented by a sha256 hash",
                }
            )
            continue
        reviews[checkpoint_id] = {
            "checkpoint_id": checkpoint_id,
            "status": status,
            "reviewer": reviewer,
            "summary_hash": summary_hash,
        }
    return reviews


def _visual_review_record(review: dict | None) -> dict:
    if not review:
        return {"status": "missing"}
    return {
        "status": review.get("status", "missing"),
        "reviewer": review.get("reviewer", "unknown"),
        "summary_hash": review.get("summary_hash", ""),
    }


def _compare_visual_region(
    checkpoint_id: str,
    original_image: dict,
    demo_image: dict,
    region: dict,
    max_mean_delta: float,
    max_changed_ratio: float,
) -> tuple[dict, list[dict]]:
    diagnostics = []
    region_id = str(region.get("region_id", "unknown"))
    x = int(region.get("x", 0))
    y = int(region.get("y", 0))
    width = int(region.get("width", 0))
    height = int(region.get("height", 0))
    required = bool(region.get("required", True))
    original_crop = _rgba_region(original_image, x, y, width, height)
    demo_crop = _rgba_region(demo_image, x, y, width, height)
    if original_crop is None or demo_crop is None:
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_COMPARISON_REGION_BOUNDS",
                "checkpoint_id": checkpoint_id,
                "region_id": region_id,
                "message": "visual comparison region is outside screenshot bounds",
            }
        )
        return (
            {
                "region_id": region_id,
                "status": "blocked",
                "mean_delta": 0.0,
                "changed_ratio": 1.0,
                "original_hash": "",
                "demo_hash": "",
                "width": width,
                "height": height,
            },
            diagnostics,
        )
    mean_delta, changed_ratio = _rgba_delta_metrics(original_crop, demo_crop)
    status = "pass" if mean_delta <= max_mean_delta and changed_ratio <= max_changed_ratio else "blocked"
    if required and status != "pass":
        diagnostics.append(
            {
                "code": "TSUI_VISUAL_COMPARISON_REGION_DIFF",
                "checkpoint_id": checkpoint_id,
                "region_id": region_id,
                "mean_delta": round(mean_delta, 4),
                "changed_ratio": round(changed_ratio, 4),
                "message": "visual region differs beyond acceptance thresholds",
            }
        )
    return (
        {
            "region_id": region_id,
            "status": status,
            "mean_delta": round(mean_delta, 4),
            "changed_ratio": round(changed_ratio, 4),
            "original_hash": _sha256_bytes(original_crop),
            "demo_hash": _sha256_bytes(demo_crop),
            "width": width,
            "height": height,
        },
        diagnostics,
    )
