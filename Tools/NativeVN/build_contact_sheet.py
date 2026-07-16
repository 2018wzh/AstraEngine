#!/usr/bin/env python3
"""Build a labeled visual-review contact sheet using optional Pillow."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from common import Diagnostics, ToolFailure, require_within


def _load_pillow():
    try:
        from PIL import Image, ImageDraw, ImageFont
    except ImportError as error:
        raise ToolFailure(
            "NATIVEVN_PILLOW_MISSING",
            "build_contact_sheet.py requires optional dependency Pillow; install it with 'python -m pip install Pillow'",
        ) from error
    return Image, ImageDraw, ImageFont


def build_contact_sheet(input_root: Path, output: Path, *, columns: int = 4, cell_width: int = 480, cell_height: int = 300) -> int:
    if columns < 1 or cell_width < 128 or cell_height < 128:
        raise ToolFailure("NATIVEVN_CONTACT_SHEET_LAYOUT_INVALID", "contact-sheet layout values are too small or invalid")
    Image, ImageDraw, ImageFont = _load_pillow()
    candidates = sorted(path for path in input_root.rglob("*.png") if ".local" not in path.parts)
    if not candidates:
        raise ToolFailure("NATIVEVN_CONTACT_SHEET_INPUT_EMPTY", "no public PNG images were found for the contact sheet")
    output_root = input_root / ".local"
    require_within(output, output_root, "NATIVEVN_CONTACT_SHEET_OUTPUT_INVALID")
    rows = (len(candidates) + columns - 1) // columns
    label_height = 34
    sheet = Image.new("RGB", (columns * cell_width, rows * (cell_height + label_height)), (18, 20, 24))
    draw = ImageDraw.Draw(sheet)
    font = ImageFont.load_default()
    for index, path in enumerate(candidates):
        with Image.open(path) as source:
            source.load()
            preview = source.convert("RGBA")
            preview.thumbnail((cell_width - 20, cell_height - 20), Image.Resampling.LANCZOS)
            backdrop = Image.new("RGB", preview.size, (42, 45, 52))
            if preview.mode == "RGBA":
                backdrop.paste(preview, mask=preview.getchannel("A"))
            else:
                backdrop.paste(preview)
        column = index % columns
        row = index // columns
        x = column * cell_width + (cell_width - backdrop.width) // 2
        y = row * (cell_height + label_height) + (cell_height - backdrop.height) // 2
        sheet.paste(backdrop, (x, y))
        label = path.relative_to(input_root).as_posix()
        draw.text((column * cell_width + 10, row * (cell_height + label_height) + cell_height + 9), label, fill=(230, 232, 238), font=font)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(f".{output.name}.tmp")
    sheet.save(temporary, format="PNG", optimize=True)
    temporary.replace(output)
    return len(candidates)


def _default_pack_root() -> Path:
    return Path(__file__).resolve().parents[2] / "Examples" / "NativeVN"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pack-root", type=Path, default=_default_pack_root())
    parser.add_argument("--output", type=Path)
    parser.add_argument("--columns", type=int, default=4)
    args = parser.parse_args(argv)
    pack_root = args.pack_root.resolve()
    output = args.output.resolve() if args.output else pack_root / ".local" / "review" / "contact-sheet.png"
    diagnostics = Diagnostics()
    try:
        count = build_contact_sheet(pack_root, output, columns=args.columns)
    except ToolFailure as error:
        diagnostics.error(error.code, error.message, error.path)
        diagnostics.emit_json()
        return 2
    except OSError:
        diagnostics.error("NATIVEVN_CONTACT_SHEET_IO_FAILED", "contact-sheet generation failed during an image or filesystem operation")
        diagnostics.emit_json()
        return 2
    diagnostics.emit_json(summary={"image_count": count, "output": ".local/review/contact-sheet.png"})
    return 0


if __name__ == "__main__":
    sys.exit(main())
