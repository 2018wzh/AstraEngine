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
from tsuinosora_diagnostics import _edition_fingerprint, _format_counts, _format_probe, _path_hints, _rel

__all__ = ['build_source_inventory', 'build_visual_reference_report', 'analyze_asset', 'analyze_png_asset', 'read_png', '_read_png_rgba', '_rgba_nonblank', '_rgba_region', '_rgba_delta_metrics', '_unfilter', '_paeth', '_components', '_bbox', '_bbox_dict', '_edge_padding_dict', '_quantized_rgb', '_color_distribution']


def build_source_inventory(root: Path | str, root_alias: str) -> dict:
    root = Path(root)
    files = []
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        rel = _rel(path, root)
        files.append(
            {
                "relative_path": rel,
                "size": path.stat().st_size,
                "sha256": _sha256(path),
                "extension": path.suffix.lower(),
                "format_probe": _format_probe(path),
            }
        )
    edition = _edition_fingerprint(files)
    return {
        "schema": "tsuinosora.source_inventory.v1",
        "root_alias": root_alias,
        "file_count": len(files),
        "format_counts": _format_counts(files),
        "edition_fingerprint": edition,
        "files": files,
    }


def build_visual_reference_report(
    title_png: Path | str,
    game_png: Path | str,
    expected_hashes: dict[str, str] | None = None,
    expected_dimensions: dict[str, dict[str, int]] | None = None,
) -> dict:
    refs = []
    diagnostics = []
    expected_hashes = expected_hashes or {}
    expected_dimensions = expected_dimensions or {}
    for logical_id, path, regions in [
        (
            "title",
            Path(title_png),
            ["title_background", "title_menu_buttons", "title_selected_state"],
        ),
        (
            "game",
            Path(game_png),
            ["background_viewport", "text_window", "speaker_name", "message_text"],
        ),
    ]:
        entry = {
            "logical_id": logical_id,
            "file_name": path.name,
            "dimensions": {"width": 0, "height": 0},
            "hash": "",
            "allowed_regions": regions,
            "report_only": [
                "hash",
                "dimensions",
                "region_id",
                "coverage",
                "diagnostic",
                "layout_metric",
            ],
        }
        if not path.is_file():
            diagnostics.append(
                {
                    "code": "TSUI_REFERENCE_MISSING",
                    "logical_id": logical_id,
                    "file_name": path.name,
                    "message": "authoritative visual reference file is missing",
                }
            )
            refs.append(entry)
            continue
        try:
            image = read_png(path)
        except (OSError, ValueError, zlib.error, struct.error):
            diagnostics.append(
                {
                    "code": "TSUI_REFERENCE_PNG_INVALID",
                    "logical_id": logical_id,
                    "file_name": path.name,
                    "message": "visual reference must be a readable PNG with supported encoding",
                }
            )
            refs.append(entry)
            continue
        dimensions = {"width": image["width"], "height": image["height"]}
        digest = _sha256(path)
        entry["dimensions"] = dimensions
        entry["hash"] = digest
        expected_digest = expected_hashes.get(logical_id, "")
        if expected_digest and digest != expected_digest:
            diagnostics.append(
                {
                    "code": "TSUI_REFERENCE_HASH_MISMATCH",
                    "logical_id": logical_id,
                    "file_name": path.name,
                    "expected_hash": expected_digest,
                    "actual_hash": digest,
                    "message": "visual reference hash does not match the authoritative evidence manifest",
                }
            )
        expected_size = expected_dimensions.get(logical_id)
        if expected_size and dimensions != expected_size:
            diagnostics.append(
                {
                    "code": "TSUI_REFERENCE_DIMENSION_MISMATCH",
                    "logical_id": logical_id,
                    "file_name": path.name,
                    "expected_dimensions": expected_size,
                    "actual_dimensions": dimensions,
                    "message": "visual reference dimensions do not match the authoritative evidence manifest",
                }
            )
        refs.append(entry)
    return {
        "schema": "tsuinosora.visual_reference_report.v1",
        "status": "blocked" if diagnostics else "pass",
        "references": refs,
        "diagnostics": diagnostics,
        "prohibited_outputs": [
            "new_commercial_screenshot",
            "commercial_text",
            "commercial_audio",
            "commercial_movie",
        ],
    }

def analyze_asset(path: Path, root: Path) -> dict:
    rel = _rel(path, root)
    ext = path.suffix.lower()
    if ext in IMAGE_EXTS:
        return analyze_png_asset(path, rel)
    if ext in AUDIO_EXTS:
        parts = {part.lower() for part in path.parts}
        classification = "voice" if parts & VOICE_HINTS else "audio"
    elif ext in MOVIE_EXTS:
        classification = "movie"
    elif ext in FONT_EXTS:
        classification = "font"
    else:
        classification = "unknown"
    return {
        "relative_path": rel,
        "classification": classification,
        "confidence": 0.9 if classification != "unknown" else 0.0,
        "sha256": _sha256(path),
    }


def analyze_png_asset(path: Path, rel: str) -> dict:
    image = read_png(path)
    mask = image["alpha_mask"]
    visible = [(x, y) for y, row in enumerate(mask) for x, value in enumerate(row) if value]
    base = {
        "relative_path": rel,
        "sha256": _sha256(path),
        "dimensions": {"width": image["width"], "height": image["height"]},
        "has_alpha": image["has_alpha"],
        "color_distribution": image["color_distribution"],
    }
    if not visible:
        return {
            **base,
            "classification": "unknown",
            "confidence": 0.0,
            "visible_bbox": None,
            "parts": [],
        }

    bbox = _bbox(visible)
    components = _components(mask)
    total_area = image["width"] * image["height"]
    bbox_area = (bbox[2] - bbox[0] + 1) * (bbox[3] - bbox[1] + 1)
    visible_ratio = len(visible) / total_area
    hints = _path_hints(rel)

    if hints["text_window"]:
        classification = "text_window"
        confidence = 0.9
        parts = []
    elif hints["button"]:
        classification = "button"
        confidence = 0.88
        parts = []
    elif hints["ui"]:
        classification = "ui"
        confidence = 0.84
        parts = []
    elif image["has_alpha"] and len(components) >= 2:
        classification = "character_atlas"
        confidence = 0.92
        parts = [
            {
                "part_id": f"part.{index:03d}",
                "pose_id": f"pose.{index:03d}",
                "expression_id": "neutral",
                "anchor": {"x": (part[0] + part[2]) // 2, "y": part[3]},
                "crop": _bbox_dict(part),
                "layer": "character",
                "mouth_eye_state_compatible": True,
                "fallback": "nearest_pose",
            }
            for index, part in enumerate(components, start=1)
        ]
    elif image["has_alpha"] and bbox_area / total_area < 0.85:
        classification = "character_sprite"
        confidence = 0.86
        parts = []
    elif not image["has_alpha"] and visible_ratio > 0.95:
        classification = "background"
        confidence = 0.88
        parts = []
    else:
        classification = "cg"
        confidence = 0.7
        parts = []

    return {
        **base,
        "classification": classification,
        "confidence": confidence,
        "visible_bbox": _bbox_dict(bbox),
        "edge_padding": _edge_padding_dict(bbox, image["width"], image["height"]),
        "component_count": len(components),
        "parts": parts,
    }


def read_png(path: Path) -> dict:
    data = path.read_bytes()
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        raise ValueError("not a PNG file")
    offset = 8
    width = height = color_type = None
    idat = bytearray()
    while offset < len(data):
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        kind = data[offset + 4 : offset + 8]
        payload = data[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            width, height, bit_depth, color_type, compression, filter_method, interlace = struct.unpack(
                ">IIBBBBB", payload
            )
            if bit_depth != 8 or compression != 0 or filter_method != 0 or interlace != 0:
                raise ValueError("unsupported PNG encoding")
            if color_type not in (2, 6):
                raise ValueError("unsupported PNG color type")
        elif kind == b"IDAT":
            idat.extend(payload)
        elif kind == b"IEND":
            break
    if width is None or height is None or color_type is None:
        raise ValueError("missing PNG IHDR")
    channels = 4 if color_type == 6 else 3
    raw = zlib.decompress(bytes(idat))
    stride = width * channels
    rows = []
    previous = [0] * stride
    cursor = 0
    for _ in range(height):
        filter_type = raw[cursor]
        cursor += 1
        encoded = list(raw[cursor : cursor + stride])
        cursor += stride
        row = _unfilter(encoded, previous, channels, filter_type)
        rows.append(row)
        previous = row
    alpha_mask = []
    has_alpha = False
    histogram = {}
    visible_count = 0
    for row in rows:
        mask_row = []
        for x in range(width):
            alpha = row[x * channels + 3] if channels == 4 else 255
            visible = alpha > 0
            mask_row.append(visible)
            has_alpha = has_alpha or alpha < 255
            if visible:
                r = row[x * channels]
                g = row[x * channels + 1]
                b = row[x * channels + 2]
                key = _quantized_rgb(r, g, b)
                histogram[key] = histogram.get(key, 0) + 1
                visible_count += 1
        alpha_mask.append(mask_row)
    return {
        "width": width,
        "height": height,
        "has_alpha": has_alpha,
        "alpha_mask": alpha_mask,
        "color_distribution": _color_distribution(histogram, visible_count),
    }


def _read_png_rgba(path: Path) -> dict:
    data = path.read_bytes()
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        raise ValueError("not a PNG file")
    offset = 8
    width = height = color_type = None
    idat = bytearray()
    while offset < len(data):
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        kind = data[offset + 4 : offset + 8]
        payload = data[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            width, height, bit_depth, color_type, compression, filter_method, interlace = struct.unpack(
                ">IIBBBBB", payload
            )
            if bit_depth != 8 or compression != 0 or filter_method != 0 or interlace != 0:
                raise ValueError("unsupported PNG encoding")
            if color_type not in (2, 6):
                raise ValueError("unsupported PNG color type")
        elif kind == b"IDAT":
            idat.extend(payload)
        elif kind == b"IEND":
            break
    if width is None or height is None or color_type is None:
        raise ValueError("missing PNG IHDR")
    channels = 4 if color_type == 6 else 3
    raw = zlib.decompress(bytes(idat))
    stride = width * channels
    rows = []
    previous = [0] * stride
    cursor = 0
    for _ in range(height):
        filter_type = raw[cursor]
        cursor += 1
        encoded = list(raw[cursor : cursor + stride])
        cursor += stride
        row = _unfilter(encoded, previous, channels, filter_type)
        rows.append(row)
        previous = row
    pixels = bytearray()
    for row in rows:
        for x in range(width):
            pixels.extend(row[x * channels : x * channels + 3])
            pixels.append(row[x * channels + 3] if channels == 4 else 255)
    return {"dimensions": {"width": width, "height": height}, "width": width, "height": height, "pixels": bytes(pixels)}


def _rgba_nonblank(pixels: bytes) -> bool:
    for offset in range(0, len(pixels), 4):
        r, g, b, a = pixels[offset : offset + 4]
        if a > 0 and (r != 0 or g != 0 or b != 0):
            return True
    return False


def _rgba_region(image: dict, x: int, y: int, width: int, height: int) -> bytes | None:
    if x < 0 or y < 0 or width <= 0 or height <= 0:
        return None
    image_width = int(image["width"])
    image_height = int(image["height"])
    if x + width > image_width or y + height > image_height:
        return None
    source = image["pixels"]
    out = bytearray()
    for row in range(y, y + height):
        start = (row * image_width + x) * 4
        out.extend(source[start : start + width * 4])
    return bytes(out)


def _rgba_delta_metrics(original: bytes, demo: bytes) -> tuple[float, float]:
    if len(original) != len(demo) or not original:
        return 255.0, 1.0
    total_delta = 0
    changed = 0
    pixels = len(original) // 4
    for offset in range(0, len(original), 4):
        pixel_changed = False
        for channel in range(3):
            delta = abs(original[offset + channel] - demo[offset + channel])
            total_delta += delta
            if delta > 8:
                pixel_changed = True
        if pixel_changed:
            changed += 1
    return total_delta / max(pixels * 3, 1), changed / max(pixels, 1)


def _unfilter(row: list[int], previous: list[int], bpp: int, filter_type: int) -> list[int]:
    out = row[:]
    for i, value in enumerate(row):
        left = out[i - bpp] if i >= bpp else 0
        up = previous[i]
        up_left = previous[i - bpp] if i >= bpp else 0
        if filter_type == 0:
            predicted = 0
        elif filter_type == 1:
            predicted = left
        elif filter_type == 2:
            predicted = up
        elif filter_type == 3:
            predicted = (left + up) // 2
        elif filter_type == 4:
            predicted = _paeth(left, up, up_left)
        else:
            raise ValueError("unsupported PNG filter")
        out[i] = (value + predicted) & 0xFF
    return out


def _paeth(left: int, up: int, up_left: int) -> int:
    prediction = left + up - up_left
    pa = abs(prediction - left)
    pb = abs(prediction - up)
    pc = abs(prediction - up_left)
    if pa <= pb and pa <= pc:
        return left
    if pb <= pc:
        return up
    return up_left


def _components(mask: list[list[bool]]) -> list[tuple[int, int, int, int]]:
    height = len(mask)
    width = len(mask[0]) if height else 0
    seen = [[False for _ in range(width)] for _ in range(height)]
    components = []
    for y in range(height):
        for x in range(width):
            if not mask[y][x] or seen[y][x]:
                continue
            queue = deque([(x, y)])
            seen[y][x] = True
            pixels = []
            while queue:
                cx, cy = queue.popleft()
                pixels.append((cx, cy))
                for nx, ny in ((cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)):
                    if 0 <= nx < width and 0 <= ny < height and mask[ny][nx] and not seen[ny][nx]:
                        seen[ny][nx] = True
                        queue.append((nx, ny))
            components.append(_bbox(pixels))
    components.sort(key=lambda box: (box[0], box[1], box[2], box[3]))
    return components


def _bbox(pixels: list[tuple[int, int]]) -> tuple[int, int, int, int]:
    xs = [pixel[0] for pixel in pixels]
    ys = [pixel[1] for pixel in pixels]
    return min(xs), min(ys), max(xs), max(ys)


def _bbox_dict(box: tuple[int, int, int, int]) -> dict:
    return {"x": box[0], "y": box[1], "width": box[2] - box[0] + 1, "height": box[3] - box[1] + 1}


def _edge_padding_dict(box: tuple[int, int, int, int], width: int, height: int) -> dict:
    return {
        "left": box[0],
        "top": box[1],
        "right": width - box[2] - 1,
        "bottom": height - box[3] - 1,
    }


def _quantized_rgb(red: int, green: int, blue: int) -> str:
    return "#{:02x}{:02x}{:02x}".format(
        (red // 64) * 64,
        (green // 64) * 64,
        (blue // 64) * 64,
    )


def _color_distribution(histogram: dict[str, int], visible_count: int) -> list[dict]:
    if visible_count == 0:
        return []
    top = sorted(histogram.items(), key=lambda item: (-item[1], item[0]))[:5]
    return [
        {
            "rgb_bin": key,
            "coverage": round(count / visible_count, 6),
        }
        for key, count in top
    ]


