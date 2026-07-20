#!/usr/bin/env python3
"""Normalize local TsuiNoSora UI references without publishing image payloads."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from dataclasses import asdict, dataclass
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


SCHEMA = "tsuinosora.original_ui_reference_manifest.v2"
TRANSFORM = "tsuinosora.reference_crop.v1"
RECAPTURE_SCHEMA = "tsuinosora.original_ui_recapture_manifest.v2"
RECAPTURE_TRANSFORM = "tsuinosora.client_capture_normalization.v2"
DESKTOP_SIZE = (3839, 2399)
CROP_BOX = (1220, 674, 2620, 1724)
OUTPUT_SIZE = (800, 600)
LEGACY_GAME_SIZE = (1403, 1053)
LEGACY_GAME_CROP = (1, 1, 1401, 1051)
CLIENT_CAPTURE_SIZE = (802, 602)
CLIENT_CAPTURE_CROP = (1, 1, 801, 601)
CLIENT_BORDER_RGBA = (100, 100, 100, 255)
NATIVE_CAPTURE_SIZE = OUTPUT_SIZE
NATIVE_CAPTURE_TRANSFORM = "tsuinosora.client_capture_identity.v1"
BORDER_CAPTURE_TRANSFORM = "tsuinosora.client_border_crop.v1"

REFERENCE_INPUTS = (
    ("TSUI1999-UI-001", "Title.png", "title"),
    ("TSUI1999-UI-002", "Opening-1.png", "opening-staggered-text"),
    ("TSUI1999-UI-003", "Opening-2.png", "opening-centered-text"),
    ("TSUI1999-UI-004", "Background.png", "stage-background"),
    ("TSUI1999-UI-005", "Stage-1.png", "dialogue-background-only"),
    ("TSUI1999-UI-006", "Stage-2.png", "dialogue-character-overflow"),
    ("TSUI1999-UI-007", "Stage-3.png", "dialogue-two-characters-overflow"),
    ("TSUI1999-UI-008", "StageHover.png", "stage-centered-monologue"),
    ("TSUI1999-UI-009", "StageHoverChoice.png", "stage-choice"),
    ("TSUI1999-UI-010", "SystemUI-1.png", "title-load"),
    ("TSUI1999-UI-011", "SystemUI-2.png", "system-popup"),
    ("TSUI1999-UI-012", "SystemUI-3.png", "system-config"),
    ("TSUI1999-UI-013", "SystemUI-4.png", "system-load"),
    ("TSUI1999-UI-014", "SystemUI-5.png", "system-save"),
)

RECAPTURE_INPUTS = (
    ("ui001.title.a", "ui001-title-a.png", "title"),
    ("ui001.title.b", "ui001-title-b.png", "title-stable"),
    ("ui005.opening.bitmap.04", "ui005-01-monologue-final.png", "opening-bitmap-04"),
    ("ui005.viewpoint", "ui005-00-viewpoint.png", "opening-viewpoint"),
    ("ui005.equation.a", "ui005-00-equation-a.png", "opening-equation"),
    ("ui005.equation.b", "ui005-00-equation-b.png", "opening-equation-stable"),
    ("ui005.dialogue.first", "ui005-02-dialogue-first.png", "dialogue-first"),
    ("ui005.dialogue.target.a", "ui005-03-target-a.png", "dialogue-target"),
    ("ui005.dialogue.target.b", "ui005-04-target-b.png", "dialogue-target-stable"),
    ("ui005.dialogue.next", "ui005-05-dialogue-next.png", "dialogue-next"),
    ("ui006.character.a", "ui006-single-character-a.png", "dialogue-character-overflow"),
    ("ui006.character.b", "ui006-single-character-b.png", "dialogue-character-overflow-stable"),
    ("ui007.two_characters.a", "ui007-two-characters-a.png", "dialogue-two-characters-overflow"),
    ("ui007.two_characters.b", "ui007-two-characters-b.png", "dialogue-two-characters-overflow-stable"),
    ("ui008.monologue.a", "ui008-monologue-a.png", "stage-centered-monologue"),
    ("ui008.monologue.b", "ui008-monologue-b.png", "stage-centered-monologue-stable"),
    ("ui009.pre_choice", "ui009-01-pre-choice.png", "choice-predecessor"),
    ("ui009.choice.a", "ui009-02-choice-a.png", "choice-target"),
    ("ui009.choice.b", "ui009-03-choice-b.png", "choice-target-stable"),
    ("ui009.post_select", "ui009-04-post-select.png", "choice-successor"),
    ("ui010.title_load.a", "ui010-title-load-a.png", "title-load"),
    ("ui010.title_load.b", "ui010-title-load-b.png", "title-load-stable"),
    ("ui011.popup.a", "ui011-story-popup-a.png", "system-popup"),
    ("ui011.popup.b", "ui011-story-popup-b.png", "system-popup-stable"),
    ("ui012.config.a", "ui012-config-a.png", "system-config"),
    ("ui012.config.b", "ui012-config-b.png", "system-config-stable"),
    ("ui013.load.a", "ui013-story-load-a.png", "system-load"),
    ("ui013.load.b", "ui013-story-load-b.png", "system-load-stable"),
    ("ui014.save.a", "ui014-story-save-a.png", "system-save"),
    ("ui014.save.b", "ui014-story-save-b.png", "system-save-stable"),
)

STABLE_RECAPTURE_PAIRS = (
    ("ui001.title.a", "ui001.title.b"),
    ("ui005.equation.a", "ui005.equation.b"),
    ("ui005.dialogue.target.a", "ui005.dialogue.target.b"),
    ("ui006.character.a", "ui006.character.b"),
    ("ui007.two_characters.a", "ui007.two_characters.b"),
    ("ui008.monologue.a", "ui008.monologue.b"),
    ("ui009.choice.a", "ui009.choice.b"),
    ("ui010.title_load.a", "ui010.title_load.b"),
    ("ui011.popup.a", "ui011.popup.b"),
    ("ui012.config.a", "ui012.config.b"),
    ("ui013.load.a", "ui013.load.b"),
    ("ui014.save.a", "ui014.save.b"),
)

CANONICAL_RECAPTURE_REFERENCES = {
    "ui001.title.a": ("TSUI1999-UI-001", "title"),
    "ui005.opening.bitmap.04": ("TSUI1999-UI-002", "opening-staggered"),
    "ui005.equation.a": ("TSUI1999-UI-003", "opening-centered"),
    "ui005.dialogue.target.a": ("TSUI1999-UI-005", "dialogue-background-only"),
    "ui006.character.a": ("TSUI1999-UI-006", "dialogue-character-overflow"),
    "ui007.two_characters.a": ("TSUI1999-UI-007", "dialogue-two-characters-overflow"),
    "ui008.monologue.a": ("TSUI1999-UI-008", "stage-centered-monologue"),
    "ui009.choice.a": ("TSUI1999-UI-009", "choice"),
    "ui010.title_load.a": ("TSUI1999-UI-010", "title-load"),
    "ui011.popup.a": ("TSUI1999-UI-011", "system-popup"),
    "ui012.config.a": ("TSUI1999-UI-012", "system-config"),
    "ui013.load.a": ("TSUI1999-UI-013", "system-load"),
    "ui014.save.a": ("TSUI1999-UI-014", "system-save"),
}

COLOR_TOLERANCE_APPROVALS = {
    "TSUI1999-UI-006": {
        "reason_code": "capture_color_state_unproven",
        "profile_id": "capture_palette_v1",
        "evidence": "same_node_resource_closure_and_stable_gpu_capture",
    }
}


class ReferenceError(RuntimeError):
    pass


@dataclass(frozen=True)
class ReferenceRecord:
    id: str
    role: str
    private_filename: str
    input_kind: str
    raw_size: list[int]
    raw_sha256: str
    crop_box: list[int] | None
    crop_sha256: str
    output_size: list[int]
    output_sha256: str


@dataclass(frozen=True)
class RecaptureRecord:
    capture_id: str
    role: str
    private_filename: str
    raw_size: list[int]
    raw_sha256: str
    normalization: str
    crop_box: list[int] | None
    output_size: list[int]
    output_sha256: str
    reference_id: str | None


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def image_size(path: Path) -> tuple[int, int]:
    with Image.open(path) as opened:
        return opened.size


def validate_client_capture(image: Image.Image, source: Path) -> None:
    if image.mode != "RGBA":
        raise ReferenceError(
            f"TSUI_RECAPTURE_MODE: {source.name} is {image.mode}, expected RGBA"
        )
    if image.size == NATIVE_CAPTURE_SIZE:
        return
    if image.size != CLIENT_CAPTURE_SIZE:
        raise ReferenceError(
            f"TSUI_RECAPTURE_SIZE: {source.name} is {image.size}, expected "
            f"{NATIVE_CAPTURE_SIZE} or {CLIENT_CAPTURE_SIZE}"
        )
    border = []
    border.extend(image.getpixel((x, 0)) for x in range(image.width))
    border.extend(image.getpixel((x, image.height - 1)) for x in range(image.width))
    border.extend(image.getpixel((0, y)) for y in range(1, image.height - 1))
    border.extend(image.getpixel((image.width - 1, y)) for y in range(1, image.height - 1))
    if any(pixel != CLIENT_BORDER_RGBA for pixel in border):
        raise ReferenceError(
            f"TSUI_RECAPTURE_BORDER: {source.name} does not have the required one-pixel border"
        )


def normalized_client_capture(
    image: Image.Image, source: Path
) -> tuple[Image.Image, str, list[int] | None]:
    validate_client_capture(image, source)
    if image.size == NATIVE_CAPTURE_SIZE:
        return image.copy(), NATIVE_CAPTURE_TRANSFORM, None
    normalized = image.crop(CLIENT_CAPTURE_CROP)
    if normalized.size != OUTPUT_SIZE:
        raise ReferenceError("TSUI_RECAPTURE_CROP_CONTRACT: normalized capture is not 800x600")
    return normalized, BORDER_CAPTURE_TRANSFORM, list(CLIENT_CAPTURE_CROP)


def normalize_client_capture(source: Path, output: Path) -> Image.Image:
    with Image.open(source) as opened:
        image = opened.copy()
    normalized, _, _ = normalized_client_capture(image, source)
    output.parent.mkdir(parents=True, exist_ok=True)
    normalized.save(output, format="PNG", optimize=True)
    return normalized


def process_recaptures(source_dir: Path, output_dir: Path) -> dict[str, object]:
    missing = [name for _, name, _ in RECAPTURE_INPUTS if not (source_dir / name).is_file()]
    if missing:
        raise ReferenceError(f"TSUI_RECAPTURE_INPUT_MISSING: {', '.join(missing)}")
    captures: list[
        tuple[str, str, str, Path, Image.Image, str, list[int] | None]
    ] = []
    normalized_payloads: dict[str, bytes] = {}
    raw_payloads: dict[str, bytes] = {}
    for capture_id, filename, role in RECAPTURE_INPUTS:
        source = source_dir / filename
        with Image.open(source) as opened:
            image = opened.copy()
        normalized, normalization, crop_box = normalized_client_capture(image, source)
        payload = normalized.tobytes()
        normalized_payloads[capture_id] = payload
        raw_payloads[capture_id] = source.read_bytes()
        captures.append(
            (capture_id, filename, role, source, normalized, normalization, crop_box)
        )
    for primary, stable in STABLE_RECAPTURE_PAIRS:
        if raw_payloads[primary] != raw_payloads[stable]:
            raise ReferenceError(
                f"TSUI_RECAPTURE_RAW_UNSTABLE: {primary} and {stable} are not byte-identical"
            )
        if normalized_payloads[primary] != normalized_payloads[stable]:
            raise ReferenceError(
                f"TSUI_RECAPTURE_UNSTABLE: {primary} and {stable} are not identical"
            )
    records: list[RecaptureRecord] = []
    canonical_reference_count = 0
    for capture_id, filename, role, source, normalized, normalization, crop_box in captures:
        target = output_dir / "normalized" / filename
        target.parent.mkdir(parents=True, exist_ok=True)
        normalized.save(target, format="PNG", optimize=True)
        canonical = CANONICAL_RECAPTURE_REFERENCES.get(capture_id)
        reference_id = canonical[0] if canonical is not None else None
        if canonical is not None:
            reference_target = output_dir / "references" / safe_private_name(*canonical)
            reference_target.parent.mkdir(parents=True, exist_ok=True)
            normalized.save(reference_target, format="PNG", optimize=True)
            canonical_reference_count += 1
        records.append(
            RecaptureRecord(
                capture_id=capture_id,
                role=role,
                private_filename=filename,
                raw_size=list(image_size(source)),
                raw_sha256=sha256(source),
                normalization=normalization,
                crop_box=crop_box,
                output_size=list(OUTPUT_SIZE),
                output_sha256=sha256(target),
                reference_id=reference_id,
            )
        )
    manifest = {
        "schema": RECAPTURE_SCHEMA,
        "transform": RECAPTURE_TRANSFORM,
        "accepted_inputs": [
            {
                "size": list(NATIVE_CAPTURE_SIZE),
                "normalization": NATIVE_CAPTURE_TRANSFORM,
                "crop_box": None,
            },
            {
                "size": list(CLIENT_CAPTURE_SIZE),
                "normalization": BORDER_CAPTURE_TRANSFORM,
                "crop_box": list(CLIENT_CAPTURE_CROP),
                "border_rgba": list(CLIENT_BORDER_RGBA),
            },
        ],
        "output_size": list(OUTPUT_SIZE),
        "resampler": "none",
        "capture_count": len(records),
        "canonical_reference_count": canonical_reference_count,
        "stable_pairs": [list(pair) for pair in STABLE_RECAPTURE_PAIRS],
        "captures": [asdict(record) for record in records],
    }
    output_dir.mkdir(parents=True, exist_ok=True)
    (output_dir / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return manifest


def public_recapture_manifest(manifest: dict[str, object]) -> dict[str, object]:
    return {
        "schema": "tsuinosora.original_ui_recapture_public_manifest.v2",
        "transform": manifest["transform"],
        "accepted_inputs": manifest["accepted_inputs"],
        "output_size": manifest["output_size"],
        "resampler": manifest["resampler"],
        "capture_count": manifest["capture_count"],
        "canonical_reference_count": manifest["canonical_reference_count"],
        "stable_pairs": manifest["stable_pairs"],
        "captures": [
            {key: value for key, value in record.items() if key != "private_filename"}
            for record in manifest["captures"]
        ],
    }


def _canonical_capture_records(manifest: dict[str, object]) -> dict[str, dict[str, object]]:
    captures = manifest.get("captures")
    if not isinstance(captures, list):
        raise ReferenceError("TSUI_RECAPTURE_MANIFEST_CAPTURES: captures must be a list")
    canonical: dict[str, dict[str, object]] = {}
    for record in captures:
        if not isinstance(record, dict):
            raise ReferenceError("TSUI_RECAPTURE_MANIFEST_CAPTURE: capture must be an object")
        reference_id = record.get("reference_id")
        if reference_id is None:
            continue
        if not isinstance(reference_id, str) or reference_id in canonical:
            raise ReferenceError(
                "TSUI_RECAPTURE_CANONICAL_DUPLICATE: canonical reference ids must be unique"
            )
        canonical[reference_id] = record
    expected = {reference_id for reference_id, _ in CANONICAL_RECAPTURE_REFERENCES.values()}
    if set(canonical) != expected:
        raise ReferenceError(
            "TSUI_RECAPTURE_CANONICAL_COVERAGE: recapture manifest must cover all RC references"
        )
    return canonical


def update_reference_manifest(
    reference_manifest: dict[str, object], recapture_manifest: dict[str, object]
) -> dict[str, object]:
    if reference_manifest.get("schema") != "tsuinosora.original_ui_reference_public_manifest.v1":
        raise ReferenceError("TSUI_REFERENCE_PUBLIC_SCHEMA: unsupported reference manifest")
    references = reference_manifest.get("references")
    if not isinstance(references, list):
        raise ReferenceError("TSUI_REFERENCE_PUBLIC_ENTRIES: references must be a list")
    canonical = _canonical_capture_records(recapture_manifest)
    seen: set[str] = set()
    updated_references: list[dict[str, object]] = []
    for entry in references:
        if not isinstance(entry, dict) or not isinstance(entry.get("id"), str):
            raise ReferenceError("TSUI_REFERENCE_PUBLIC_ENTRY: reference entry is invalid")
        reference_id = entry["id"]
        if reference_id in seen:
            raise ReferenceError("TSUI_REFERENCE_PUBLIC_DUPLICATE: reference ids must be unique")
        seen.add(reference_id)
        record = canonical.get(reference_id)
        if record is None:
            updated_references.append(dict(entry))
            continue
        normalization = record.get("normalization")
        if normalization == NATIVE_CAPTURE_TRANSFORM:
            input_kind = "client_identity"
        elif normalization == BORDER_CAPTURE_TRANSFORM:
            input_kind = "client_border_crop"
        else:
            raise ReferenceError(
                "TSUI_RECAPTURE_NORMALIZATION: canonical capture has an unknown transform"
            )
        updated = dict(entry)
        updated.update(
            {
                "input_kind": input_kind,
                "raw_size": record["raw_size"],
                "raw_sha256": record["raw_sha256"],
                "crop_box": record["crop_box"],
                "crop_sha256": record["output_sha256"],
                "output_size": record["output_size"],
                "output_sha256": record["output_sha256"],
            }
        )
        updated_references.append(updated)
    if len(seen) != 15 or not set(canonical).issubset(seen):
        raise ReferenceError(
            "TSUI_REFERENCE_PUBLIC_COVERAGE: public reference manifest must retain 15 stable ids"
        )
    updated_manifest = dict(reference_manifest)
    updated_manifest["references"] = updated_references
    return updated_manifest


def update_node_map(
    node_map: dict[str, object], recapture_manifest: dict[str, object]
) -> dict[str, object]:
    if node_map.get("schema") != "tsuinosora.classic_visual_node_map.v3":
        raise ReferenceError("TSUI_RECAPTURE_NODE_MAP_SCHEMA: unsupported node map")
    entries = node_map.get("entries")
    if not isinstance(entries, list):
        raise ReferenceError("TSUI_RECAPTURE_NODE_MAP_ENTRIES: entries must be a list")
    canonical = _canonical_capture_records(recapture_manifest)
    stable_by_primary = {primary: stable for primary, stable in STABLE_RECAPTURE_PAIRS}
    captures_by_id = {
        record["capture_id"]: record
        for record in recapture_manifest["captures"]
        if isinstance(record, dict) and isinstance(record.get("capture_id"), str)
    }
    canonical_capture_by_reference = {
        reference_id: capture_id
        for capture_id, (reference_id, _) in CANONICAL_RECAPTURE_REFERENCES.items()
    }
    seen: set[str] = set()
    updated_entries: list[dict[str, object]] = []
    for raw_entry in entries:
        if not isinstance(raw_entry, dict) or not isinstance(raw_entry.get("reference_id"), str):
            raise ReferenceError("TSUI_RECAPTURE_NODE_MAP_ENTRY: node map entry is invalid")
        entry = dict(raw_entry)
        reference_id = entry["reference_id"]
        if reference_id in seen:
            raise ReferenceError("TSUI_RECAPTURE_NODE_MAP_DUPLICATE: reference ids must be unique")
        seen.add(reference_id)
        record = canonical.get(reference_id)
        if record is not None:
            identity = entry.get("identity")
            if not isinstance(identity, dict):
                raise ReferenceError("TSUI_RECAPTURE_NODE_MAP_IDENTITY: identity is required")
            identity = dict(identity)
            identity["reference_sha256"] = "sha256:" + str(record["output_sha256"])
            entry["identity"] = identity
            capture_id = canonical_capture_by_reference[reference_id]
            stable_id = stable_by_primary.get(capture_id)
            if stable_id is not None:
                stable = captures_by_id.get(stable_id)
                if stable is None or stable.get("output_sha256") != record.get("output_sha256"):
                    raise ReferenceError(
                        "TSUI_RECAPTURE_NODE_MAP_STABILITY: stable capture evidence is inconsistent"
                    )
                entry["reference_validation"] = {
                    "status": "verified",
                    "method": "byte_identical_stable_pair",
                    "capture_pair_sha256": "sha256:" + str(record["output_sha256"]),
                }
            else:
                locator = identity.get("locator")
                resource_hashes = identity.get("resource_hashes")
                if (
                    reference_id != "TSUI1999-UI-002"
                    or not isinstance(locator, dict)
                    or locator.get("method") != "score_bitmap_text"
                    or not isinstance(locator.get("content_sha256"), str)
                    or not isinstance(resource_hashes, list)
                    or locator["content_sha256"] not in resource_hashes
                ):
                    raise ReferenceError(
                        "TSUI_RECAPTURE_NODE_MAP_STABILITY: canonical capture lacks stable evidence"
                    )
                entry["reference_validation"] = {
                    "status": "verified",
                    "method": "score_bitmap_resource_closure",
                    "capture_sha256": "sha256:" + str(record["output_sha256"]),
                    "resource_sha256": locator["content_sha256"],
                }
            color_approval = COLOR_TOLERANCE_APPROVALS.get(reference_id)
            if color_approval is not None:
                entry["color_tolerance_approval"] = dict(color_approval)
        updated_entries.append(entry)
    if len(seen) != 15 or not set(canonical).issubset(seen):
        raise ReferenceError(
            "TSUI_RECAPTURE_NODE_MAP_COVERAGE: node map must retain all 15 reference ids"
        )
    updated_map = dict(node_map)
    updated_map["entries"] = updated_entries
    return updated_map


def write_json_atomic(path: Path, value: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".partial")
    temporary.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    temporary.replace(path)


def safe_private_name(reference_id: str, role: str) -> str:
    return f"tsui1999-ui-{reference_id[-3:].lower()}-{role}.png"


def reference_source(source_dir: Path, reference_id: str, original_name: str, role: str) -> Path:
    original = source_dir / original_name
    if original.is_file():
        return original
    normalized_private_name = source_dir / safe_private_name(reference_id, role)
    if normalized_private_name.is_file():
        return normalized_private_name
    stable_id_matches = sorted(source_dir.glob(f"tsui1999-ui-{reference_id[-3:].lower()}-*.png"))
    if len(stable_id_matches) == 1:
        return stable_id_matches[0]
    if len(stable_id_matches) > 1:
        raise ReferenceError(
            f"TSUI_REFERENCE_STABLE_ID_AMBIGUOUS: {reference_id} has multiple private inputs"
        )
    return original


def validate_desktop_image(image: Image.Image, source: Path) -> None:
    if image.size != DESKTOP_SIZE:
        raise ReferenceError(
            f"TSUI_REFERENCE_DESKTOP_SIZE: {source.name} is {image.size}, expected {DESKTOP_SIZE}"
        )
    crop_width = CROP_BOX[2] - CROP_BOX[0]
    crop_height = CROP_BOX[3] - CROP_BOX[1]
    if (crop_width, crop_height) != (1400, 1050):
        raise ReferenceError("TSUI_REFERENCE_CROP_CONTRACT: crop is not 1400x1050")
    if CROP_BOX[2] > image.width or CROP_BOX[3] > image.height:
        raise ReferenceError(f"TSUI_REFERENCE_CROP_BOUNDS: crop exceeds {source.name}")
    validate_content_region(image, source)


def _non_black_ratio(image: Image.Image, box: tuple[int, int, int, int]) -> float:
    sample = image.crop(box).convert("RGB")
    non_black = sum(1 for pixel in sample.getdata() if max(pixel) > 12)
    return non_black / (sample.width * sample.height)


def validate_content_region(image: Image.Image, source: Path) -> None:
    edge_contracts = {
        "left": ((1180, 674, 1220, 1724), (0.070, 0.080)),
        "right": ((2620, 674, 2660, 1724), (0.045, 0.055)),
        "top": ((1220, 634, 2620, 674), (0.070, 0.080)),
        "bottom": ((1220, 1724, 2620, 1764), (0.045, 0.055)),
    }
    for edge, (box, bounds) in edge_contracts.items():
        ratio = _non_black_ratio(image, box)
        if not bounds[0] <= ratio <= bounds[1]:
            raise ReferenceError(
                f"TSUI_REFERENCE_CONTENT_REGION_DRIFT: {source.name} {edge} edge ratio {ratio:.6f}"
            )
    if _non_black_ratio(image, CROP_BOX) < 0.003:
        raise ReferenceError(
            f"TSUI_REFERENCE_CONTENT_REGION_EMPTY: {source.name} has no observable game content"
        )


def normalize_desktop(source: Path, raw_target: Path, crop_target: Path, output: Path) -> ReferenceRecord:
    with Image.open(source) as opened:
        image = opened.convert("RGB")
    validate_desktop_image(image, source)
    raw_target.parent.mkdir(parents=True, exist_ok=True)
    crop_target.parent.mkdir(parents=True, exist_ok=True)
    output.parent.mkdir(parents=True, exist_ok=True)
    image.save(raw_target, format="PNG", optimize=True)
    crop = image.crop(CROP_BOX)
    crop.save(crop_target, format="PNG", optimize=True)
    normalized = crop.resize(OUTPUT_SIZE, Image.Resampling.LANCZOS)
    normalized.save(output, format="PNG", optimize=True)
    return image, crop


def normalize_legacy_game(repository: Path, output_dir: Path) -> ReferenceRecord:
    payload = subprocess.run(
        ["git", "show", "HEAD:Examples/TsuiNoSora/Docs/Game.png"],
        cwd=repository,
        check=True,
        capture_output=True,
    ).stdout
    raw = output_dir / "raw" / "tsui1999-ui-015-legacy-game.png"
    raw.parent.mkdir(parents=True, exist_ok=True)
    raw.write_bytes(payload)
    with Image.open(raw) as opened:
        image = opened.convert("RGB")
    if image.size != LEGACY_GAME_SIZE:
        raise ReferenceError(
            f"TSUI_REFERENCE_LEGACY_SIZE: legacy Game.png is {image.size}, expected {LEGACY_GAME_SIZE}"
        )
    cropped = image.crop(LEGACY_GAME_CROP)
    output = output_dir / "normalized" / "tsui1999-ui-015-legacy-game.png"
    output.parent.mkdir(parents=True, exist_ok=True)
    cropped.resize(OUTPUT_SIZE, Image.Resampling.LANCZOS).save(output, format="PNG", optimize=True)
    return ReferenceRecord(
        id="TSUI1999-UI-015",
        role="legacy-game",
        private_filename=output.name,
        input_kind="legacy_normalized",
        raw_size=list(image.size),
        raw_sha256=sha256(raw),
        crop_box=list(LEGACY_GAME_CROP),
        crop_sha256=hashlib.sha256(cropped.tobytes()).hexdigest(),
        output_size=list(OUTPUT_SIZE),
        output_sha256=sha256(output),
    )


def make_contact_sheet(records: list[ReferenceRecord], output_dir: Path) -> None:
    thumb_size = (320, 240)
    columns = 3
    rows = (len(records) + columns - 1) // columns
    canvas = Image.new("RGB", (columns * 340, rows * 280), "#202020")
    draw = ImageDraw.Draw(canvas)
    font = ImageFont.load_default()
    for index, record in enumerate(records):
        source = output_dir / "normalized" / record.private_filename
        with Image.open(source) as opened:
            thumb = opened.convert("RGB").resize(thumb_size, Image.Resampling.LANCZOS)
        x = (index % columns) * 340 + 10
        y = (index // columns) * 280 + 10
        canvas.paste(thumb, (x, y))
        draw.text((x, y + 246), f"{record.id} {record.role}", fill="white", font=font)
    canvas.save(output_dir / "tsui1999-ui-reference-contact-sheet.png", optimize=True)


def public_manifest(manifest: dict[str, object]) -> dict[str, object]:
    return {
        "schema": "tsuinosora.original_ui_reference_public_manifest.v1",
        "transform": manifest["transform"],
        "desktop_size": manifest["desktop_size"],
        "crop_box": manifest["crop_box"],
        "output_size": manifest["output_size"],
        "resampler": manifest["resampler"],
        "reference_count": manifest["reference_count"],
        "references": [
            {
                "id": record["id"],
                "role": record["role"],
                "input_kind": record["input_kind"],
                "raw_size": record["raw_size"],
                "raw_sha256": record["raw_sha256"],
                "crop_box": record["crop_box"],
                "crop_sha256": record["crop_sha256"],
                "output_size": record["output_size"],
                "output_sha256": record["output_sha256"],
            }
            for record in manifest["references"]
        ],
    }


def process(repository: Path, source_dir: Path, output_dir: Path) -> dict[str, object]:
    missing = [
        name
        for reference_id, name, role in REFERENCE_INPUTS
        if not reference_source(source_dir, reference_id, name, role).is_file()
    ]
    if missing:
        raise ReferenceError(f"TSUI_REFERENCE_INPUT_MISSING: {', '.join(missing)}")
    records: list[ReferenceRecord] = []
    raw_sizes: set[tuple[int, int]] = set()
    for reference_id, filename, role in REFERENCE_INPUTS:
        source = reference_source(source_dir, reference_id, filename, role)
        private_name = safe_private_name(reference_id, role)
        raw_target = output_dir / "raw" / private_name
        crop_target = output_dir / "cropped" / private_name
        normalized_target = output_dir / "normalized" / private_name
        normalized_target.parent.mkdir(parents=True, exist_ok=True)
        image, _ = normalize_desktop(source, raw_target, crop_target, normalized_target)
        raw_sizes.add(image.size)
        records.append(
            ReferenceRecord(
                id=reference_id,
                role=role,
                private_filename=private_name,
                input_kind="desktop_crop",
                raw_size=list(image.size),
                raw_sha256=sha256(raw_target),
                crop_box=list(CROP_BOX),
                crop_sha256=sha256(crop_target),
                output_size=list(OUTPUT_SIZE),
                output_sha256=sha256(normalized_target),
            )
        )
    if raw_sizes != {DESKTOP_SIZE}:
        raise ReferenceError("TSUI_REFERENCE_CONTENT_REGION_DRIFT: desktop source sizes disagree")
    records.append(normalize_legacy_game(repository, output_dir))
    make_contact_sheet(records, output_dir)
    manifest = {
        "schema": SCHEMA,
        "transform": TRANSFORM,
        "desktop_size": list(DESKTOP_SIZE),
        "crop_box": list(CROP_BOX),
        "output_size": list(OUTPUT_SIZE),
        "resampler": "pillow_lanczos",
        "reference_count": len(records),
        "references": [asdict(record) for record in records],
    }
    (output_dir / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", type=Path, default=Path.cwd())
    parser.add_argument("--source-dir", type=Path)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--public-manifest", type=Path)
    parser.add_argument("--reference-manifest", type=Path)
    parser.add_argument("--node-map", type=Path)
    parser.add_argument("--recapture-dir", type=Path)
    args = parser.parse_args()
    if (args.source_dir is None) == (args.recapture_dir is None):
        parser.error("exactly one of --source-dir and --recapture-dir is required")
    if args.recapture_dir is not None:
        manifest = process_recaptures(args.recapture_dir.resolve(), args.output_dir.resolve())
        public = public_recapture_manifest(manifest)
        if (args.reference_manifest is None) != (args.node_map is None):
            parser.error("--reference-manifest and --node-map must be supplied together")
        updated_reference_manifest = None
        updated_node_map = None
        if args.reference_manifest is not None:
            updated_reference_manifest = update_reference_manifest(
                json.loads(args.reference_manifest.read_text(encoding="utf-8")), manifest
            )
            updated_node_map = update_node_map(
                json.loads(args.node_map.read_text(encoding="utf-8")), manifest
            )
    else:
        if args.reference_manifest is not None or args.node_map is not None:
            parser.error("public evidence synchronization is only valid for --recapture-dir")
        manifest = process(args.repository.resolve(), args.source_dir.resolve(), args.output_dir.resolve())
        public = public_manifest(manifest)
    if args.public_manifest is not None:
        write_json_atomic(args.public_manifest, public)
    if args.recapture_dir is not None and args.reference_manifest is not None:
        write_json_atomic(args.reference_manifest, updated_reference_manifest)
        write_json_atomic(args.node_map, updated_node_map)
    count = manifest.get("reference_count", manifest.get("capture_count"))
    print(json.dumps({"schema": manifest["schema"], "count": count}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
