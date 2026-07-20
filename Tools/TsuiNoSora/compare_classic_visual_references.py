#!/usr/bin/env python3
"""Fail-closed same-node Classic visual comparison and review artifact builder."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw


POLICY_SCHEMA = "tsuinosora.classic_visual_comparison_policy.v3"
NODE_MAP_SCHEMA = "tsuinosora.classic_visual_node_map.v3"
REPORT_SCHEMA = "tsuinosora.classic_visual_comparison_report.v3"
CAPTURE_NORMALIZATION_ID = "windows_175pct_bilinear_then_lanczos_v1"
LEGACY_DESKTOP_CAPTURE_REFERENCE_IDS = {
    "TSUI1999-UI-001",
    "TSUI1999-UI-002",
    "TSUI1999-UI-004",
    "TSUI1999-UI-006",
    "TSUI1999-UI-007",
    "TSUI1999-UI-008",
    "TSUI1999-UI-010",
    "TSUI1999-UI-011",
    "TSUI1999-UI-012",
    "TSUI1999-UI-013",
    "TSUI1999-UI-014",
    "TSUI1999-UI-015",
}
COLOR_TOLERANCE_PROFILE_ID = "capture_palette_v1"
COLOR_TOLERANCE_PROFILE = {
    "reason_code": "capture_color_state_unproven",
    "min_ssim": 0.75,
    "max_perceptual_error": 0.12,
}
TOLERANCE_APPROVAL_SCHEMA = "astra.headless_tolerance_approval.v2"


class ComparisonError(RuntimeError):
    pass


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return f"sha256:{digest.hexdigest()}"


def _json_hash(value: object) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def _image(path: Path) -> np.ndarray:
    with Image.open(path) as opened:
        if opened.size != (800, 600):
            raise ComparisonError(f"TSUI_CLASSIC_VISUAL_SIZE: {path.name} is not 800x600")
        return np.asarray(opened.convert("RGB"), dtype=np.float64)


def _capture_normalization(policy: dict) -> tuple[set[str], str]:
    normalization = policy.get("capture_normalization")
    expected_ids = LEGACY_DESKTOP_CAPTURE_REFERENCE_IDS
    if not isinstance(normalization, dict) or (
        normalization.get("id") != CAPTURE_NORMALIZATION_ID
        or normalization.get("source_size") != [800, 600]
        or normalization.get("captured_size") != [1400, 1050]
        or normalization.get("upscale") != "bilinear"
        or normalization.get("downscale") != "lanczos"
        or set(normalization.get("reference_ids", [])) != expected_ids
    ):
        raise ComparisonError("TSUI_CLASSIC_VISUAL_CAPTURE_NORMALIZATION: unsupported policy")
    return expected_ids, CAPTURE_NORMALIZATION_ID


def _color_tolerance_profiles(policy: dict) -> dict[str, dict]:
    profiles = policy.get("color_tolerance_profiles")
    expected = {COLOR_TOLERANCE_PROFILE_ID: COLOR_TOLERANCE_PROFILE}
    if profiles != expected:
        raise ComparisonError(
            "TSUI_CLASSIC_VISUAL_COLOR_TOLERANCE_POLICY: unsupported color tolerance profiles"
        )
    return profiles


def _color_tolerance_approval(
    policy: dict, profiles: dict[str, dict], policy_root: Path | None
) -> dict[str, str]:
    binding = policy.get("color_tolerance_approval")
    if not isinstance(binding, dict) or set(binding) != {"relative_path", "sha256"}:
        raise ComparisonError(
            "TSUI_CLASSIC_VISUAL_COLOR_TOLERANCE_APPROVAL: a hash-bound human approval is required"
        )
    relative_path = binding.get("relative_path")
    expected_sha256 = binding.get("sha256")
    if not isinstance(relative_path, str) or not isinstance(expected_sha256, str):
        raise ComparisonError(
            "TSUI_CLASSIC_VISUAL_COLOR_TOLERANCE_APPROVAL: approval binding is malformed"
        )
    relative = Path(relative_path)
    if policy_root is None or relative.is_absolute() or any(part in {"", ".", ".."} for part in relative.parts):
        raise ComparisonError(
            "TSUI_CLASSIC_VISUAL_COLOR_TOLERANCE_APPROVAL: approval path must be safe and policy-relative"
        )
    root = policy_root.resolve()
    approval_path = (root / relative).resolve()
    if not approval_path.is_relative_to(root) or not approval_path.is_file():
        raise ComparisonError(
            "TSUI_CLASSIC_VISUAL_COLOR_TOLERANCE_APPROVAL: approval record is missing"
        )
    actual_sha256 = _sha256(approval_path)
    if actual_sha256 != expected_sha256:
        raise ComparisonError(
            "TSUI_CLASSIC_VISUAL_COLOR_TOLERANCE_APPROVAL: approval hash mismatch"
        )
    approval = json.loads(approval_path.read_text(encoding="utf-8"))
    if (
        set(approval) != {
            "schema",
            "approval_id",
            "approver_kind",
            "approver_identity",
            "approved_tolerance_hash",
            "previous_config_hash",
            "reason_codes",
        }
        or approval.get("schema") != TOLERANCE_APPROVAL_SCHEMA
        or approval.get("approver_kind") != "human"
        or not isinstance(approval.get("approval_id"), str)
        or not approval.get("approval_id")
        or not isinstance(approval.get("approver_identity"), str)
        or not approval.get("approver_identity")
        or approval.get("approved_tolerance_hash") != _json_hash(profiles)
        or approval.get("previous_config_hash") is not None
        or approval.get("reason_codes") != [COLOR_TOLERANCE_PROFILE["reason_code"]]
    ):
        raise ComparisonError(
            "TSUI_CLASSIC_VISUAL_COLOR_TOLERANCE_APPROVAL: approval record does not authorize this profile"
        )
    return {
        "approval_id": approval["approval_id"],
        "approval_sha256": actual_sha256,
        "approved_tolerance_hash": approval["approved_tolerance_hash"],
    }


def _normalize_capture(path: Path, enabled: bool) -> np.ndarray:
    with Image.open(path) as opened:
        if opened.size != (800, 600):
            raise ComparisonError(f"TSUI_CLASSIC_VISUAL_SIZE: {path.name} is not 800x600")
        image = opened.convert("RGB")
    if enabled:
        image = image.resize((1400, 1050), Image.Resampling.BILINEAR).resize(
            (800, 600), Image.Resampling.LANCZOS
        )
    return np.asarray(image, dtype=np.float64)


def _box(value: object, code: str) -> list[int]:
    if not isinstance(value, list) or len(value) != 4 or any(type(item) is not int for item in value):
        raise ComparisonError(f"{code}: expected an integer rectangle")
    left, top, right, bottom = value
    if not (0 <= left < right <= 800 and 0 <= top < bottom <= 600):
        raise ComparisonError(f"{code}: rectangle is out of bounds")
    return value


def _region(image: np.ndarray, box: list[int]) -> np.ndarray:
    left, top, right, bottom = _box(box, "TSUI_CLASSIC_VISUAL_REGION")
    return image[top:bottom, left:right]


def _mask(shape: tuple[int, int], boxes: list[list[int]], max_coverage: float) -> np.ndarray:
    if not 0.0 <= max_coverage <= 0.35:
        raise ComparisonError("TSUI_CLASSIC_VISUAL_MASK_POLICY: max coverage must be within 0..0.35")
    result = np.zeros(shape, dtype=bool)
    for value in boxes:
        left, top, right, bottom = _box(value, "TSUI_CLASSIC_VISUAL_MASK_RECT")
        result[top:bottom, left:right] = True
    coverage = float(result.mean())
    if coverage > max_coverage:
        raise ComparisonError(
            f"TSUI_CLASSIC_VISUAL_MASK_EXCESSIVE: {coverage:.6f} exceeds {max_coverage:.6f}"
        )
    return result


def _masked_values(left: np.ndarray, right: np.ndarray, excluded: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    included = ~excluded
    if int(included.sum()) < 4096:
        raise ComparisonError("TSUI_CLASSIC_VISUAL_MASK_EMPTY: fewer than 4096 pixels remain")
    return left[included], right[included]


def _ssim(left: np.ndarray, right: np.ndarray, excluded: np.ndarray | None = None) -> float:
    left_luma = left.mean(axis=2)
    right_luma = right.mean(axis=2)
    if excluded is not None:
        left_luma, right_luma = _masked_values(left_luma, right_luma, excluded)
    left_mean, right_mean = left_luma.mean(), right_luma.mean()
    left_var, right_var = left_luma.var(), right_luma.var()
    covariance = ((left_luma - left_mean) * (right_luma - right_mean)).mean()
    c1, c2 = (0.01 * 255) ** 2, (0.03 * 255) ** 2
    return float(
        ((2 * left_mean * right_mean + c1) * (2 * covariance + c2))
        / ((left_mean**2 + right_mean**2 + c1) * (left_var + right_var + c2))
    )


def _perceptual_error(left: np.ndarray, right: np.ndarray, excluded: np.ndarray | None = None) -> float:
    weights = np.array([0.2126, 0.7152, 0.0722])
    delta = np.abs((left * weights).sum(axis=2) - (right * weights).sum(axis=2))
    if excluded is not None:
        delta = delta[~excluded]
    return float(delta.mean() / 255.0)


def _bbox_delta(left: list[int], right: list[int]) -> int:
    return max(abs(a - b) for a, b in zip(left, right))


def _light_text_bbox(image: np.ndarray, search: list[int]) -> list[int]:
    left, top, right, bottom = _box(search, "TSUI_CLASSIC_VISUAL_TEXT_SEARCH")
    sample = image[top:bottom, left:right]
    luminance = sample.mean(axis=2)
    chroma = sample.max(axis=2) - sample.min(axis=2)
    ys, xs = np.where((luminance >= 145) & (chroma <= 85))
    if len(xs) < 12:
        raise ComparisonError("TSUI_CLASSIC_VISUAL_TEXT_MISSING: bounded light text was not found")
    return [left + int(xs.min()), top + int(ys.min()), left + int(xs.max() + 1), top + int(ys.max() + 1)]


def _white_diamond_origins(
    image: np.ndarray, search: list[int], expected_count: int
) -> list[int]:
    left, top, right, bottom = _box(search, "TSUI_CLASSIC_VISUAL_DIAMOND_SEARCH")
    sample = image[top:bottom, left:right]
    luminance = sample.mean(axis=2)
    chroma = sample.max(axis=2) - sample.min(axis=2)
    mask = (luminance >= 205) & (chroma <= 32)
    # The original diamonds are compact; text strokes are rejected by their filled 8..24 px box.
    candidates: list[list[int]] = []
    visited = np.zeros(mask.shape, dtype=bool)
    for y, x in zip(*np.where(mask)):
        if visited[y, x]:
            continue
        stack = [(int(x), int(y))]
        visited[y, x] = True
        component: list[tuple[int, int]] = []
        while stack:
            cx, cy = stack.pop()
            component.append((cx, cy))
            for nx, ny in ((cx - 1, cy), (cx + 1, cy), (cx, cy - 1), (cx, cy + 1)):
                if 0 <= nx < mask.shape[1] and 0 <= ny < mask.shape[0] and mask[ny, nx] and not visited[ny, nx]:
                    visited[ny, nx] = True
                    stack.append((nx, ny))
        xs, ys = zip(*component)
        width, height = max(xs) - min(xs) + 1, max(ys) - min(ys) + 1
        fill_ratio = len(component) / (width * height)
        if (
            8 <= width <= 24
            and 8 <= height <= 24
            and len(component) >= 24
            and 0.30 <= fill_ratio <= 0.75
        ):
            candidates.append([left + min(xs), top + min(ys), left + max(xs) + 1, top + max(ys) + 1])
    if not candidates:
        raise ComparisonError("TSUI_CLASSIC_VISUAL_CHOICE_DIAMOND_MISSING: white choice diamond was not found")
    rows: list[list[list[int]]] = []
    for candidate in sorted(candidates, key=lambda item: (item[1], item[0])):
        if not rows or candidate[1] - rows[-1][0][1] > 8:
            rows.append([candidate])
        else:
            rows[-1].append(candidate)
    if len(rows) != expected_count:
        raise ComparisonError(
            "TSUI_CLASSIC_VISUAL_CHOICE_DIAMOND_COUNT: the authored diamond count is not exact"
        )
    # Japanese option glyphs can contain another compact diamond-like component.
    # The authored marker is the leftmost component on each option row and must
    # form one stable column.  This also rejects the former double-marker bug:
    # after the first row, the embedded Director glyph became the leftmost item.
    authored = [min(row, key=lambda item: item[0]) for row in rows]
    authored_x = [item[0] for item in authored]
    if max(authored_x) - min(authored_x) > 2:
        raise ComparisonError(
            "TSUI_CLASSIC_VISUAL_CHOICE_DIAMOND_COUNT: authored diamonds do not form one column"
        )
    origins = [tuple(item[:2]) for item in authored]
    return [
        min(origin[0] for origin in origins),
        min(origin[1] for origin in origins),
        max(origin[0] for origin in origins),
        max(origin[1] for origin in origins),
    ]


def _largest_gray_component(image: np.ndarray) -> list[int]:
    luminance = image.mean(axis=2)
    gray = (image.max(axis=2) - image.min(axis=2) <= 24) & (luminance >= 120) & (luminance <= 248)
    # Director system windows occupy the bounded center of the 800x600 stage.
    candidates: list[tuple[int, int, int]] = []
    for y in range(120, 480):
        xs = np.where(gray[y, 180:620])[0] + 180
        if not len(xs):
            continue
        start = previous = int(xs[0])
        runs: list[tuple[int, int]] = []
        for value in xs[1:]:
            x = int(value)
            if x != previous + 1:
                runs.append((start, previous + 1))
                start = x
            previous = x
        runs.append((start, previous + 1))
        for left, right in runs:
            center = (left + right) / 2.0
            if 300 <= right - left <= 350 and 372 <= center <= 428:
                candidates.append((y, left, right))
    if not candidates:
        raise ComparisonError("TSUI_CLASSIC_VISUAL_MODAL_MISSING: gray modal was not found")
    centers = np.asarray([((left + right) / 2.0, y) for y, left, right in candidates])
    center_x = float(np.median(centers[:, 0]))
    matching_rows = sorted({y for y, _, _ in candidates})
    groups: list[list[int]] = []
    for y in matching_rows:
        if not groups or y - groups[-1][-1] > 140:
            groups.append([y])
        else:
            groups[-1].append(y)
    group = max(groups, key=lambda item: (item[-1] - item[0], len(item)))
    if len(group) < 20 or group[-1] - group[0] < 180:
        raise ComparisonError("TSUI_CLASSIC_VISUAL_MODAL_DISCONNECTED: modal rows are disconnected")
    center_y = (group[0] + group[-1] + 1) / 2.0
    # Skin raster is exempt. Compare the detected center using the authored 336x242 Score extent.
    left = int(round(center_x - 168))
    top = int(round(center_y - 121))
    return [left, top, left + 336, top + 242]


def _geometry_metrics(reference: np.ndarray, capture: np.ndarray, regions: list[dict]) -> tuple[list[dict], int]:
    metrics = []
    maximum = 0
    for region in regions:
        region_id = str(region.get("id", ""))
        if not region_id:
            raise ComparisonError("TSUI_CLASSIC_VISUAL_GEOMETRY_ID: geometry region id is required")
        detector = region.get("detector")
        if detector == "modal_bbox":
            reference_rect = _largest_gray_component(reference)
            capture_rect = _largest_gray_component(capture)
        elif detector == "light_text_bbox":
            search = _box(region.get("search"), "TSUI_CLASSIC_VISUAL_TEXT_SEARCH")
            reference_rect = _light_text_bbox(reference, search)
            capture_rect = _light_text_bbox(capture, search)
        elif detector == "light_text_origin":
            search = _box(region.get("search"), "TSUI_CLASSIC_VISUAL_TEXT_SEARCH")
            reference_bbox = _light_text_bbox(reference, search)
            capture_bbox = _light_text_bbox(capture, search)
            reference_rect = [reference_bbox[0], reference_bbox[1]] * 2
            capture_rect = [capture_bbox[0], capture_bbox[1]] * 2
        elif detector == "choice_diamond_origins":
            search = _box(region.get("search"), "TSUI_CLASSIC_VISUAL_DIAMOND_SEARCH")
            expected_count = region.get("expected_count")
            if type(expected_count) is not int or not 1 <= expected_count <= 32:
                raise ComparisonError(
                    "TSUI_CLASSIC_VISUAL_CHOICE_DIAMOND_POLICY: expected_count is required"
                )
            reference_rect = _white_diamond_origins(reference, search, expected_count)
            capture_rect = _white_diamond_origins(capture, search, expected_count)
        elif detector == "declared_rect":
            reference_rect = _box(region.get("reference_rect"), "TSUI_CLASSIC_VISUAL_REFERENCE_RECT")
            capture_rect = _box(region.get("capture_rect"), "TSUI_CLASSIC_VISUAL_CAPTURE_RECT")
        else:
            raise ComparisonError(f"TSUI_CLASSIC_VISUAL_GEOMETRY_DETECTOR: unsupported {detector}")
        delta = _bbox_delta(reference_rect, capture_rect)
        maximum = max(maximum, delta)
        metrics.append(
            {"id": region_id, "reference_rect": reference_rect, "capture_rect": capture_rect, "delta_px": delta}
        )
    return metrics, maximum


def _review_artifacts(
    root: Path,
    check_id: str,
    reference: np.ndarray,
    capture: np.ndarray,
    excluded: np.ndarray,
) -> dict[str, str]:
    target = root / check_id
    target.mkdir(parents=True, exist_ok=False)
    reference_u8 = reference.astype(np.uint8)
    capture_u8 = capture.astype(np.uint8)
    mask_u8 = np.where(excluded, 255, 0).astype(np.uint8)
    absolute = np.abs(reference - capture).clip(0, 255).astype(np.uint8)
    weighted = np.abs(
        (reference * np.array([0.2126, 0.7152, 0.0722])).sum(axis=2)
        - (capture * np.array([0.2126, 0.7152, 0.0722])).sum(axis=2)
    )
    heat = np.zeros((600, 800, 3), dtype=np.uint8)
    heat[..., 0] = np.clip(weighted * 3.0, 0, 255).astype(np.uint8)
    heat[..., 1] = np.clip((weighted - 32) * 1.5, 0, 180).astype(np.uint8)
    heat[excluded] = [72, 72, 72]
    files = {
        "reference": target / "reference.png",
        "capture": target / "capture.png",
        "mask": target / "mask.png",
        "absolute_diff": target / "absolute-diff.png",
        "perceptual_heatmap": target / "perceptual-heatmap.png",
    }
    Image.fromarray(reference_u8).save(files["reference"])
    Image.fromarray(capture_u8).save(files["capture"])
    Image.fromarray(mask_u8, mode="L").save(files["mask"])
    Image.fromarray(absolute).save(files["absolute_diff"])
    Image.fromarray(heat).save(files["perceptual_heatmap"])
    sheet = Image.new("RGB", (1600, 1800), (24, 24, 24))
    draw = ImageDraw.Draw(sheet)
    panels = [
        ("reference", reference_u8, 0, 0),
        ("capture", capture_u8, 800, 0),
        ("mask", np.repeat(mask_u8[..., None], 3, axis=2), 0, 600),
        ("absolute diff", absolute, 800, 600),
        ("perceptual heatmap", heat, 400, 1200),
    ]
    for label, pixels, x, y in panels:
        sheet.paste(Image.fromarray(pixels), (x, y))
        draw.rectangle((x, y, x + 210, y + 22), fill=(0, 0, 0))
        draw.text((x + 5, y + 4), label, fill=(255, 255, 255))
    sheet_path = target / "five-panel.png"
    sheet.save(sheet_path)
    files["five_panel"] = sheet_path
    return {name: _sha256(path) for name, path in files.items()}


def _entries_by_id(node_map: dict) -> dict[str, dict]:
    if node_map.get("schema") != NODE_MAP_SCHEMA:
        raise ComparisonError("TSUI_CLASSIC_VISUAL_NODE_MAP_SCHEMA: unsupported node map")
    result: dict[str, dict] = {}
    checkpoints: set[str] = set()
    for entry in node_map.get("entries", []):
        reference_id = str(entry.get("reference_id", ""))
        checkpoint = str(entry.get("checkpoint", ""))
        if not reference_id or reference_id in result or not checkpoint or checkpoint in checkpoints:
            raise ComparisonError("TSUI_CLASSIC_VISUAL_NODE_MAP_UNIQUE: references and checkpoints must be unique")
        identity = entry.get("identity")
        if entry.get("comparison_class") == "same_node":
            required = ("movie_id", "frame", "typed_state", "wait_command", "handler_id", "reference_sha256")
            if not isinstance(identity, dict) or any(identity.get(key) in (None, "") for key in required):
                raise ComparisonError("TSUI_CLASSIC_VISUAL_NODE_IDENTITY: same-node identity is incomplete")
            resources = identity.get("resource_hashes")
            if not isinstance(resources, list) or not resources:
                raise ComparisonError("TSUI_CLASSIC_VISUAL_RESOURCE_CLOSURE: same-node resource closure is empty")
            locator = identity.get("locator")
            if (
                not isinstance(locator, dict)
                or locator.get("method")
                not in {
                    "system_resource",
                    "score_bitmap_text",
                    "resource_sequence",
                    "story_text",
                    "story_choice",
                }
                or not isinstance(locator.get("content_sha256"), str)
                or not locator["content_sha256"].startswith("sha256:")
            ):
                raise ComparisonError(
                    "TSUI_CLASSIC_VISUAL_TEXT_LOCATOR: same-node text/resource locator is incomplete"
                )
            if locator["method"] in {"story_text", "story_choice"}:
                candidates = locator.get("candidate_commands")
                if (
                    not isinstance(candidates, list)
                    or not candidates
                    or len(candidates) != len(set(candidates))
                    or identity["wait_command"] not in candidates
                ):
                    raise ComparisonError(
                        "TSUI_CLASSIC_VISUAL_TEXT_LOCATOR: story locator candidates are incomplete"
                    )
            reference_validation = entry.get("reference_validation", {"status": "verified"})
            if reference_validation.get("status") not in {
                "verified",
                "recapture_required",
            }:
                raise ComparisonError("TSUI_CLASSIC_VISUAL_REFERENCE_VALIDATION: unsupported status")
            if reference_validation.get("status") == "verified" and "method" in reference_validation:
                validation_method = reference_validation.get("method")
                if validation_method == "byte_identical_stable_pair":
                    valid_reference = (
                        isinstance(reference_validation.get("capture_pair_sha256"), str)
                        and len(reference_validation["capture_pair_sha256"]) == 71
                        and reference_validation["capture_pair_sha256"]
                        == identity["reference_sha256"]
                    )
                elif validation_method == "score_bitmap_resource_closure":
                    locator = identity.get("locator")
                    resource_hashes = identity.get("resource_hashes")
                    valid_reference = (
                        reference_id == "TSUI1999-UI-002"
                        and reference_validation.get("capture_sha256")
                        == identity["reference_sha256"]
                        and isinstance(locator, dict)
                        and locator.get("method") == "score_bitmap_text"
                        and reference_validation.get("resource_sha256")
                        == locator.get("content_sha256")
                        and isinstance(resource_hashes, list)
                        and reference_validation.get("resource_sha256") in resource_hashes
                    )
                else:
                    valid_reference = False
                if not valid_reference:
                    raise ComparisonError(
                        "TSUI_CLASSIC_VISUAL_REFERENCE_VALIDATION: reference stability evidence is incomplete"
                    )
            if reference_validation.get("status") == "recapture_required":
                if reference_validation.get("reason_code") not in {
                    "source_presentation_contradiction",
                    "capture_color_state_unproven",
                    "capture_transition_state_unproven",
                } or reference_validation.get("required_evidence") != "two_consecutive_frames":
                    raise ComparisonError(
                        "TSUI_CLASSIC_VISUAL_REFERENCE_VALIDATION: recapture evidence is incomplete"
                    )
            color_approval = entry.get("color_tolerance_approval")
            if color_approval is not None:
                if (
                    not isinstance(color_approval, dict)
                    or color_approval.get("reason_code")
                    != COLOR_TOLERANCE_PROFILE["reason_code"]
                    or color_approval.get("profile_id") != COLOR_TOLERANCE_PROFILE_ID
                    or color_approval.get("evidence")
                    != "same_node_resource_closure_and_stable_gpu_capture"
                ):
                    raise ComparisonError(
                        "TSUI_CLASSIC_VISUAL_REFERENCE_VALIDATION: color tolerance approval is incomplete"
                    )
        result[reference_id] = entry
        checkpoints.add(checkpoint)
    return result


def compare(
    policy: dict,
    node_map: dict,
    acceptance: dict,
    references: Path,
    captures: Path,
    artifacts: Path,
    policy_root: Path | None = None,
) -> dict:
    if policy.get("schema") != POLICY_SCHEMA:
        raise ComparisonError("TSUI_CLASSIC_VISUAL_POLICY_SCHEMA: unsupported policy")
    thresholds = policy.get("thresholds", {})
    max_geometry = thresholds.get("max_geometry_delta_px")
    min_ssim = thresholds.get("min_ssim")
    max_perceptual = thresholds.get("max_perceptual_error")
    if max_geometry != 2 or min_ssim != 0.94 or max_perceptual != 0.08:
        raise ComparisonError("TSUI_CLASSIC_VISUAL_FIXED_THRESHOLDS: v3 thresholds are fixed")
    color_profiles = _color_tolerance_profiles(policy)
    color_tolerance_approval = _color_tolerance_approval(policy, color_profiles, policy_root)
    normalized_reference_ids, normalization_id = _capture_normalization(policy)
    entries = _entries_by_id(node_map)
    if acceptance.get("schema") != "tsuinosora.classic_visual_acceptance_report.v2" or acceptance.get("status") != "passed":
        raise ComparisonError("TSUI_CLASSIC_VISUAL_ACCEPTANCE_REPORT: a passed v2 GPU report is required")
    locator_evidence = [
        {
            "reference_id": entry.get("reference_id"),
            "typed_state": entry.get("identity", {}).get("typed_state"),
            "wait_command": entry.get("identity", {}).get("wait_command"),
            "locator": entry.get("identity", {}).get("locator"),
        }
        for entry in node_map.get("entries", [])
        if entry.get("comparison_class") == "same_node"
    ]
    if acceptance.get("text_locator_evidence_hash") != _json_hash(locator_evidence):
        raise ComparisonError(
            "TSUI_CLASSIC_VISUAL_TEXT_LOCATOR: GPU acceptance is not bound to the node map"
        )
    observed_nodes: dict[str, dict] = {}
    for run in acceptance.get("runs", []):
        for checkpoint, identity in run.get("checkpoint_nodes", {}).items():
            if checkpoint in observed_nodes:
                raise ComparisonError("TSUI_CLASSIC_VISUAL_ACCEPTANCE_DUPLICATE: checkpoint evidence is duplicated")
            observed_nodes[checkpoint] = identity
    artifacts.mkdir(parents=True, exist_ok=False)
    results = []
    diagnostics = []
    seen: set[str] = set()
    for check in policy.get("checks", []):
        check_id = str(check.get("id", ""))
        reference_id = str(check.get("reference_id", ""))
        if not check_id or check_id in seen:
            raise ComparisonError("TSUI_CLASSIC_VISUAL_POLICY_ID: check ids must be unique")
        seen.add(check_id)
        entry = entries.get(reference_id)
        if entry is None or entry.get("checkpoint") != check.get("checkpoint"):
            raise ComparisonError("TSUI_CLASSIC_VISUAL_NODE_POLICY_BINDING: policy and node map disagree")
        expected_observation = {
            "reference_id": reference_id,
            "typed_state": entry.get("identity", {}).get("typed_state"),
            "wait_command": entry.get("identity", {}).get("wait_command"),
        }
        if entry.get("comparison_class") == "same_node":
            if observed_nodes.get(check["checkpoint"]) != expected_observation:
                diagnostics.append(
                    {
                        "code": "TSUI_CLASSIC_VISUAL_INPUT_EVIDENCE",
                        "check_id": check_id,
                        "capture": "primary",
                    }
                )
                continue
            stable_id = f"{check['checkpoint']}.__stable"
            if observed_nodes.get(stable_id) != expected_observation:
                diagnostics.append(
                    {
                        "code": "TSUI_CLASSIC_VISUAL_INPUT_EVIDENCE",
                        "check_id": check_id,
                        "capture": "stable",
                    }
                )
                continue
        reference_matches = list(references.glob(f"tsui1999-ui-{reference_id[-3:]}-*.png"))
        capture_matches = list(captures.rglob(f"{check['checkpoint']}.png"))
        if len(reference_matches) != 1 or len(capture_matches) != 1:
            diagnostics.append({"code": "TSUI_CLASSIC_VISUAL_INPUT_MISSING", "check_id": check_id})
            continue
        reference_path, capture_path = reference_matches[0], capture_matches[0]
        expected_hash = entry.get("identity", {}).get("reference_sha256")
        if expected_hash and _sha256(reference_path) != expected_hash:
            diagnostics.append({"code": "TSUI_CLASSIC_VISUAL_REFERENCE_HASH", "check_id": check_id})
            continue
        reference = _image(reference_path)
        capture = _normalize_capture(capture_path, reference_id in normalized_reference_ids)
        if entry.get("comparison_class") == "same_node":
            stable_matches = list(captures.rglob(f"{check['checkpoint']}.__stable.png"))
            if len(stable_matches) != 1 or _sha256(capture_path) != _sha256(stable_matches[0]):
                diagnostics.append({"code": "TSUI_CLASSIC_VISUAL_UNSTABLE_CAPTURE", "check_id": check_id})
                continue
        mask_boxes = check.get("mask", {}).get("boxes", [])
        excluded = _mask((600, 800), mask_boxes, check.get("mask", {}).get("max_coverage", 0.0))
        metrics: dict[str, object] = {
            "mask_coverage": round(float(excluded.mean()), 6),
            "ssim": round(_ssim(reference, capture, excluded), 6),
            "perceptual_error": round(_perceptual_error(reference, capture, excluded), 6),
        }
        geometry, geometry_delta = _geometry_metrics(reference, capture, check.get("geometry", []))
        metrics["geometry"] = geometry
        metrics["max_geometry_delta_px"] = geometry_delta
        comparison_class = entry.get("comparison_class")
        reference_validation = entry.get("reference_validation", {"status": "verified"})
        color_approval = entry.get("color_tolerance_approval")
        color_tolerance_id = check.get("color_tolerance")
        color_tolerance = None
        if color_tolerance_id is not None:
            if color_tolerance_id not in color_profiles:
                raise ComparisonError(
                    "TSUI_CLASSIC_VISUAL_COLOR_TOLERANCE_POLICY: check references an unknown profile"
                )
            if (
                comparison_class != "same_node"
                or not isinstance(color_approval, dict)
                or color_approval.get("profile_id") != color_tolerance_id
            ):
                raise ComparisonError(
                    "TSUI_CLASSIC_VISUAL_COLOR_TOLERANCE_BINDING: tolerance is not approved by node evidence"
                )
            color_tolerance = color_profiles[color_tolerance_id]
        elif color_approval is not None:
            raise ComparisonError(
                "TSUI_CLASSIC_VISUAL_COLOR_TOLERANCE_BINDING: approved node is missing a policy profile"
            )
        if comparison_class == "same_node":
            if color_tolerance is not None:
                passed = (
                    geometry_delta <= max_geometry
                    and metrics["ssim"] >= color_tolerance["min_ssim"]
                    and metrics["perceptual_error"]
                    <= color_tolerance["max_perceptual_error"]
                )
            else:
                passed = (
                    reference_validation["status"] == "verified"
                    and geometry_delta <= max_geometry
                    and metrics["ssim"] >= min_ssim
                    and metrics["perceptual_error"] <= max_perceptual
                )
        elif comparison_class == "system_geometry":
            # Font raster and widget skin are exempt; geometry remains strict.
            passed = geometry_delta <= max_geometry
        else:
            raise ComparisonError("TSUI_CLASSIC_VISUAL_COMPARISON_CLASS: unsupported comparison class")
        artifact_hashes = _review_artifacts(artifacts, check_id, reference, capture, excluded)
        results.append(
            {
                "id": check_id,
                "reference_id": reference_id,
                "checkpoint": check["checkpoint"],
                "comparison_class": comparison_class,
                "reference_validation": reference_validation,
                "status": "pass" if passed else "blocked",
                "node_identity_hash": "sha256:"
                + hashlib.sha256(json.dumps(entry["identity"], sort_keys=True).encode()).hexdigest(),
                "reference_sha256": _sha256(reference_path),
                "capture_sha256": _sha256(capture_path),
                "capture_normalization": normalization_id
                if reference_id in normalized_reference_ids
                else "none",
                "color_tolerance": {
                    "profile_id": color_tolerance_id,
                    "applied": color_tolerance is not None,
                    "thresholds": color_tolerance,
                },
                "metrics": metrics,
                "review_artifact_hashes": artifact_hashes,
            }
        )
        if not passed:
            diagnostics.append(
                {
                    "code": "TSUI_CLASSIC_VISUAL_REFERENCE_RECAPTURE_REQUIRED"
                    if reference_validation["status"] == "recapture_required"
                    else "TSUI_CLASSIC_VISUAL_THRESHOLD",
                    "check_id": check_id,
                }
            )
    return {
        "schema": REPORT_SCHEMA,
        "status": "pass" if len(results) == len(policy.get("checks", [])) and not diagnostics else "blocked",
        "check_count": len(results),
        "passed_count": sum(result["status"] == "pass" for result in results),
        "passed_with_color_tolerance_count": sum(
            result["status"] == "pass" and result["color_tolerance"]["applied"]
            for result in results
        ),
        "thresholds": thresholds,
        "color_tolerance_profiles": color_profiles,
        "color_tolerance_approval": color_tolerance_approval,
        "results": results,
        "diagnostics": diagnostics,
        "redaction": {"paths": "omitted", "payload": "omitted", "commercial_text": "omitted"},
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--policy", type=Path, required=True)
    parser.add_argument("--node-map", type=Path, required=True)
    parser.add_argument("--acceptance-report", type=Path, required=True)
    parser.add_argument("--references", type=Path, required=True)
    parser.add_argument("--captures", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    report = compare(
        json.loads(args.policy.read_text(encoding="utf-8")),
        json.loads(args.node_map.read_text(encoding="utf-8")),
        json.loads(args.acceptance_report.read_text(encoding="utf-8")),
        args.references,
        args.captures,
        args.artifacts,
        args.policy.parent,
    )
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"schema": report["schema"], "status": report["status"], "passed": report["passed_count"], "total": report["check_count"]}))
    return 0 if report["status"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
