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
DESKTOP_SIZE = (3839, 2399)
CROP_BOX = (1220, 674, 2620, 1724)
OUTPUT_SIZE = (800, 600)
LEGACY_GAME_SIZE = (1403, 1053)
LEGACY_GAME_CROP = (1, 1, 1401, 1051)

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


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


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
    parser.add_argument("--source-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--public-manifest", type=Path)
    args = parser.parse_args()
    manifest = process(args.repository.resolve(), args.source_dir.resolve(), args.output_dir.resolve())
    if args.public_manifest is not None:
        args.public_manifest.parent.mkdir(parents=True, exist_ok=True)
        args.public_manifest.write_text(
            json.dumps(public_manifest(manifest), ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
    print(json.dumps({"schema": manifest["schema"], "reference_count": manifest["reference_count"]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
