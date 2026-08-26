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

__all__ = ['_visual_capture_project_resolution', '_visual_capture_project_scale_filter', '_normalize_visual_capture_image', '_visual_nonblank_bbox', '_rgba_crop_bytes', '_resize_rgba_bytes', '_resize_rgba_nearest', '_resize_rgba_linear']


def _visual_capture_project_resolution(work_root: Path) -> tuple[int, int] | None:
    project = work_root / "nativevn" / "project.yaml"
    if not project.is_file():
        return None
    try:
        text = project.read_text(encoding="utf-8")
    except OSError:
        return None
    match = re.search(
        r"original_resolution:\s*\n\s*width:\s*(\d+)\s*\n\s*height:\s*(\d+)",
        text,
    )
    if not match:
        return None
    width = int(match.group(1))
    height = int(match.group(2))
    if 0 < width <= 16384 and 0 < height <= 16384:
        return (width, height)
    return None


def _visual_capture_project_scale_filter(work_root: Path) -> str:
    project = work_root / "nativevn" / "project.yaml"
    if not project.is_file():
        return "linear"
    try:
        text = project.read_text(encoding="utf-8")
    except OSError:
        return "linear"
    match = re.search(r"scale_filter:\s*([A-Za-z0-9_-]+)", text)
    value = match.group(1) if match else "linear"
    return value if value in {"nearest", "linear"} else "linear"

def _normalize_visual_capture_image(image: dict, resolution: tuple[int, int] | None, scale_filter: str) -> dict:
    if not resolution:
        return image
    target_width, target_height = resolution
    width = int(image.get("width", 0))
    height = int(image.get("height", 0))
    rgba = image.get("rgba", b"")
    if width == target_width and height == target_height:
        return image
    if width <= 0 or height <= 0 or len(rgba) != width * height * 4:
        return image
    crop = _visual_nonblank_bbox(rgba, width, height)
    if crop is None:
        return image
    x, y, crop_width, crop_height = crop
    if crop_width >= target_width and crop_height >= target_height and (
        crop_width - target_width <= 4 and crop_height - target_height <= 4
    ):
        x += (crop_width - target_width) // 2
        y += (crop_height - target_height) // 2
        crop_width = target_width
        crop_height = target_height
    cropped = _rgba_crop_bytes(rgba, width, height, x, y, crop_width, crop_height)
    if crop_width != target_width or crop_height != target_height:
        cropped = _resize_rgba_bytes(cropped, crop_width, crop_height, target_width, target_height, scale_filter)
    return {"width": target_width, "height": target_height, "rgba": cropped}


def _visual_nonblank_bbox(rgba: bytes, width: int, height: int) -> tuple[int, int, int, int] | None:
    min_x = width
    min_y = height
    max_x = -1
    max_y = -1
    for y in range(height):
        row = y * width * 4
        for x in range(width):
            offset = row + x * 4
            r, g, b, a = rgba[offset : offset + 4]
            if a and (r > 8 or g > 8 or b > 8):
                min_x = min(min_x, x)
                min_y = min(min_y, y)
                max_x = max(max_x, x)
                max_y = max(max_y, y)
    if max_x < min_x or max_y < min_y:
        return None
    return (min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)


def _rgba_crop_bytes(rgba: bytes, width: int, height: int, x: int, y: int, crop_width: int, crop_height: int) -> bytes:
    if x < 0 or y < 0 or crop_width <= 0 or crop_height <= 0 or x + crop_width > width or y + crop_height > height:
        return rgba
    out = bytearray()
    stride = width * 4
    row_len = crop_width * 4
    for row in range(y, y + crop_height):
        start = row * stride + x * 4
        out.extend(rgba[start : start + row_len])
    return bytes(out)


def _resize_rgba_bytes(
    rgba: bytes,
    width: int,
    height: int,
    target_width: int,
    target_height: int,
    scale_filter: str,
) -> bytes:
    if scale_filter == "nearest":
        return _resize_rgba_nearest(rgba, width, height, target_width, target_height)
    return _resize_rgba_linear(rgba, width, height, target_width, target_height)


def _resize_rgba_nearest(rgba: bytes, width: int, height: int, target_width: int, target_height: int) -> bytes:
    out = bytearray(target_width * target_height * 4)
    for y in range(target_height):
        src_y = min(height - 1, int((y + 0.5) * height / target_height))
        for x in range(target_width):
            src_x = min(width - 1, int((x + 0.5) * width / target_width))
            src = (src_y * width + src_x) * 4
            dst = (y * target_width + x) * 4
            out[dst : dst + 4] = rgba[src : src + 4]
    return bytes(out)


def _resize_rgba_linear(rgba: bytes, width: int, height: int, target_width: int, target_height: int) -> bytes:
    if target_width == 1:
        x_scale = 0.0
    else:
        x_scale = (width - 1) / (target_width - 1)
    if target_height == 1:
        y_scale = 0.0
    else:
        y_scale = (height - 1) / (target_height - 1)
    out = bytearray(target_width * target_height * 4)
    for y in range(target_height):
        src_y = y * y_scale
        y0 = int(src_y)
        y1 = min(height - 1, y0 + 1)
        wy = src_y - y0
        for x in range(target_width):
            src_x = x * x_scale
            x0 = int(src_x)
            x1 = min(width - 1, x0 + 1)
            wx = src_x - x0
            dst = (y * target_width + x) * 4
            for channel in range(4):
                p00 = rgba[(y0 * width + x0) * 4 + channel]
                p10 = rgba[(y0 * width + x1) * 4 + channel]
                p01 = rgba[(y1 * width + x0) * 4 + channel]
                p11 = rgba[(y1 * width + x1) * 4 + channel]
                top = p00 * (1.0 - wx) + p10 * wx
                bottom = p01 * (1.0 - wx) + p11 * wx
                out[dst + channel] = round(top * (1.0 - wy) + bottom * wy)
    return bytes(out)
