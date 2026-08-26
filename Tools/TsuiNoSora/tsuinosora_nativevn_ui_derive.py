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
from tsuinosora_nativevn_font import _write_asset_sidecar

__all__ = ['_director_runtime_bindings', '_derive_director_solid_black', '_derive_director_character_sprite', '_derive_director_dialogue_frame', '_derive_director_background_transparent_sprite', '_copy_classic_ui_assets', '_walk_director_operations']

def _director_runtime_bindings(binding_ir: dict):
    for scene in binding_ir.get("scenes", []):
        for operation in _walk_director_operations(scene.get("operations", [])):
            role = None
            if operation.get("kind") == "show_member":
                role = str(operation.get("layer", ""))
            elif operation.get("kind") == "show_eye":
                role = "eye"
            yield operation.get("binding"), role
    for opening in binding_ir.get("score_openings", []):
        if not isinstance(opening, dict):
            raise ValueError("Director score opening must be an object")
        for frame in opening.get("frames", []):
            if not isinstance(frame, dict):
                raise ValueError("Director score opening frame must be an object")
            sprite = frame.get("sprite")
            if sprite is not None:
                if not isinstance(sprite, dict):
                    raise ValueError("Director score opening sprite must be an object")
                yield sprite.get("binding"), "event"
    for stage_layout in binding_ir.get("stage_layouts", []):
        layers = stage_layout.get("layers") if isinstance(stage_layout, dict) else None
        if not isinstance(layers, dict):
            raise ValueError("Director stage layout must contain typed layers")
        for layer_name, layer in layers.items():
            if not isinstance(layer, dict):
                raise ValueError("Director stage layout layer must be an object")
            binding = layer.get("binding")
            role = str(layer_name)
            if isinstance(binding, dict) and binding.get("director_member") == "black":
                role = "solid_black"
            yield binding, role


def _derive_director_solid_black(source: Path, target: Path) -> bool:
    """Recover Director's palette-backed full-frame ``black`` cast member.

    ProjectorRays exports this indexed BITD as an opaque white RGB image because the
    Director palette is external to the standalone payload.  The caller only assigns
    the ``solid_black`` role after an exact Score-to-cast binding proves the member name.
    The pixel and geometry checks below prevent this transform from accepting an
    arbitrary image that merely happens to be used on a character channel.
    """
    try:
        from PIL import Image
    except ImportError as error:
        raise RuntimeError("Director solid palette recovery requires Pillow") from error

    with Image.open(source) as opened:
        rgba = opened.convert("RGBA")
    if rgba.size != (800, 600):
        return False
    colors = rgba.getcolors(maxcolors=2)
    if colors != [(800 * 600, (255, 255, 255, 255))]:
        return False
    Image.new("RGBA", rgba.size, (0, 0, 0, 255)).save(target)
    return True


def _derive_director_character_sprite(source: Path, target: Path) -> bool:
    """Recover Director's bounded white-matte sprite from ProjectorRays PNG output."""
    try:
        from PIL import Image
    except ImportError as error:
        raise RuntimeError("Director character matte recovery requires Pillow") from error

    with Image.open(source) as opened:
        rgba = opened.convert("RGBA")
    if rgba.size != (802, 602):
        return False
    width, height = rgba.size
    outer = [
        rgba.getpixel((0, 0)),
        rgba.getpixel((width - 1, 0)),
        rgba.getpixel((0, height - 1)),
        rgba.getpixel((width - 1, height - 1)),
    ]
    if any(pixel != (0, 0, 0, 255) for pixel in outer):
        return False
    inner = [
        rgba.getpixel((1, 1)),
        rgba.getpixel((width - 2, 1)),
        rgba.getpixel((1, height - 2)),
        rgba.getpixel((width - 2, height - 2)),
    ]
    white_corners = sum(pixel == (255, 255, 255, 255) for pixel in inner)
    if white_corners < 3:
        return False

    sprite = rgba.crop((1, 1, width - 1, height - 1))
    pixels = sprite.load()
    sprite_width, sprite_height = sprite.size
    queue: deque[tuple[int, int]] = deque()
    visited: set[tuple[int, int]] = set()
    for x in range(sprite_width):
        queue.append((x, 0))
        queue.append((x, sprite_height - 1))
    for y in range(sprite_height):
        queue.append((0, y))
        queue.append((sprite_width - 1, y))
    while queue:
        x, y = queue.popleft()
        if (x, y) in visited:
            continue
        r, g, b, alpha = pixels[x, y]
        if alpha != 255 or max(255 - r, 255 - g, 255 - b) > 24:
            continue
        visited.add((x, y))
        pixels[x, y] = (r, g, b, 0)
        if x:
            queue.append((x - 1, y))
        if x + 1 < sprite_width:
            queue.append((x + 1, y))
        if y:
            queue.append((x, y - 1))
        if y + 1 < sprite_height:
            queue.append((x, y + 1))
    if len(visited) < sprite_width * sprite_height // 10:
        raise ValueError("Director character white matte is not a bounded edge region")
    target.parent.mkdir(parents=True, exist_ok=True)
    sprite.save(target, format="PNG", compress_level=9, optimize=False)
    return True


def _derive_director_dialogue_frame(source: Path, target: Path) -> bool:
    """Restore the translucent paper field encoded by Director's dialogue cast."""
    try:
        from PIL import Image
    except ImportError as error:
        raise RuntimeError("Director dialogue derivation requires Pillow") from error

    with Image.open(source) as opened:
        rgba = opened.convert("RGBA")
    if rgba.size != (754, 82):
        return False
    pixels = list(rgba.getdata())
    paper = sum(1 for red, green, blue, alpha in pixels if alpha == 255 and min(red, green, blue) >= 248)
    if paper < len(pixels) * 3 // 4:
        raise ValueError("Director dialogue frame does not contain the proven bounded paper field")
    derived = []
    for red, green, blue, alpha in pixels:
        if alpha == 255 and min(red, green, blue) >= 248:
            derived.append((red, green, blue, 160))
        else:
            derived.append((red, green, blue, alpha))
    rgba.putdata(derived)
    target.parent.mkdir(parents=True, exist_ok=True)
    rgba.save(target, format="PNG", compress_level=9, optimize=False)
    return True


def _derive_director_background_transparent_sprite(source: Path, target: Path) -> bool:
    """Recover Director background-transparent ink from an edge-connected white matte."""
    try:
        from PIL import Image
    except ImportError as error:
        raise RuntimeError("Director background-transparent ink recovery requires Pillow") from error

    with Image.open(source) as opened:
        rgba = opened.convert("RGBA")
    width, height = rgba.size
    if width <= 1 or height <= 1 or rgba.size in {(800, 600), (802, 602)}:
        return False
    corners = (
        rgba.getpixel((0, 0)),
        rgba.getpixel((width - 1, 0)),
        rgba.getpixel((0, height - 1)),
        rgba.getpixel((width - 1, height - 1)),
    )
    if sum(
        alpha == 255 and max(255 - red, 255 - green, 255 - blue) <= 24
        for red, green, blue, alpha in corners
    ) < 3:
        return False

    pixels = rgba.load()
    queue: deque[tuple[int, int]] = deque()
    visited: set[tuple[int, int]] = set()
    for x in range(width):
        queue.extend(((x, 0), (x, height - 1)))
    for y in range(height):
        queue.extend(((0, y), (width - 1, y)))
    while queue:
        x, y = queue.popleft()
        if (x, y) in visited:
            continue
        red, green, blue, alpha = pixels[x, y]
        if alpha != 255 or max(255 - red, 255 - green, 255 - blue) > 24:
            continue
        visited.add((x, y))
        pixels[x, y] = (red, green, blue, 0)
        if x:
            queue.append((x - 1, y))
        if x + 1 < width:
            queue.append((x + 1, y))
        if y:
            queue.append((x, y - 1))
        if y + 1 < height:
            queue.append((x, y + 1))
    if len(visited) < width * height // 10:
        raise ValueError("Director background-transparent matte is not a bounded edge region")
    target.parent.mkdir(parents=True, exist_ok=True)
    rgba.save(target, format="PNG", compress_level=9, optimize=False)
    return True


def _copy_classic_ui_assets(
    work_root: Path,
    nativevn_root: Path,
    resources: dict[str, dict],
) -> None:
    required = (
        (
            "native-assets/projectorrays/data/MENU/chunks/BITD-444.png",
            "native-assets/ui/classic/frame.png",
            "tsui.ui.classic.frame",
            "sha256:6c945086d7e1160ac374e8e9f32e03a4282466b99685f0ade7545ace72861b88",
            "copy",
        ),
        (
            "native-assets/projectorrays/casts/GENERAL/GENERAL/chunks/BITD-1283.png",
            "native-assets/ui/classic/dialogue.png",
            "tsui.ui.classic.dialogue",
            "sha256:7e68165e5d8783fc5950dff1a8b1164c2e91dd0a9937af5bf8bd2acf94ecf3a5",
            "dialogue",
        ),
        (
            "native-assets/projectorrays/data/MENU/chunks/BITD-449.png",
            "native-assets/ui/classic/menu-save.png",
            "tsui.ui.classic.menu.save",
            "sha256:24633ae07b6e48d684509ddcebb17417cdc166248034f2c91b91a9847620ed52",
            "copy",
        ),
        (
            "native-assets/projectorrays/data/MENU/chunks/BITD-454.png",
            "native-assets/ui/classic/menu-load.png",
            "tsui.ui.classic.menu.load",
            "sha256:7147403eb63c2234c45c5c7df24c388cf454b9096e68f3e75e4953ced9930ed3",
            "copy",
        ),
        (
            "native-assets/projectorrays/data/MENU/chunks/BITD-455.png",
            "native-assets/ui/classic/menu-exit.png",
            "tsui.ui.classic.menu.exit",
            "sha256:3aef150616889ae240f8cf04e3eab6100c734d3334eadf59d76cd2fddf15f4e1",
            "copy",
        ),
    )
    for source_path, target_path, asset_id, expected_hash, transform in required:
        resource = resources.get(source_path)
        if not isinstance(resource, dict) or resource.get("converted_hash") != expected_hash:
            raise ValueError("classic UI asset identity does not match the reviewed conversion")
        source = work_root / source_path
        if not source.is_file() or _sha256(source) != expected_hash:
            raise FileNotFoundError("reviewed classic UI asset is missing or has changed")
        target = nativevn_root / target_path
        target.parent.mkdir(parents=True, exist_ok=True)
        if transform == "dialogue":
            if not _derive_director_dialogue_frame(source, target):
                raise ValueError("reviewed classic dialogue asset has an unexpected geometry")
        else:
            shutil.copy2(source, target)
        ui_resource = {
            **resource,
            "native_path": target_path,
            "classification": "ui",
            "converted_hash": _sha256(target),
        }
        _write_asset_sidecar(target, target_path, ui_resource, asset_id)


def _walk_director_operations(operations):
    for operation in operations:
        if not isinstance(operation, dict):
            raise ValueError("Director asset binding operation must be an object")
        yield operation
        for key in ("operations", "events"):
            children = operation.get(key)
            if isinstance(children, list):
                yield from _walk_director_operations(children)
