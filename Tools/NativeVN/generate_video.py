#!/usr/bin/env python3
"""Build the deterministic 12-second rain-signal loop and layered sources."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import shutil
import subprocess
import sys
from pathlib import Path

try:
    from PIL import Image, ImageDraw, ImageFilter
except ImportError as error:  # pragma: no cover - environment preflight
    raise SystemExit("NATIVEVN_VIDEO_PIL_MISSING: install Pillow before rebuilding video") from error


WIDTH = 1920
HEIGHT = 1080
FPS = 24
DURATION_SECONDS = 12
FRAME_COUNT = FPS * DURATION_SECONDS
SEED = 0x4E415456


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _rain_streaks() -> list[tuple[int, int, int, int]]:
    state = SEED
    streaks: list[tuple[int, int, int, int]] = []
    for _ in range(260):
        state = (1664525 * state + 1013904223) & 0xFFFFFFFF
        x = state % WIDTH
        state = (1664525 * state + 1013904223) & 0xFFFFFFFF
        y = state % HEIGHT
        state = (1664525 * state + 1013904223) & 0xFFFFFFFF
        length = 18 + state % 72
        state = (1664525 * state + 1013904223) & 0xFFFFFFFF
        alpha = 24 + state % 48
        streaks.append((x, y, length, alpha))
    return streaks


def _write_layers(source_dir: Path, background: Path) -> tuple[Image.Image, list[tuple[int, int, int, int]]]:
    source_dir.mkdir(parents=True, exist_ok=True)
    with Image.open(background) as opened:
        base = opened.convert("RGB").resize((WIDTH, HEIGHT), Image.Resampling.LANCZOS)
    base.save(source_dir / "base.png", optimize=True)

    streaks = _rain_streaks()
    rain = Image.new("RGBA", (WIDTH, HEIGHT), (0, 0, 0, 0))
    rain_draw = ImageDraw.Draw(rain)
    for x, y, length, alpha in streaks:
        rain_draw.line((x, y, x - 5, y + length), fill=(145, 211, 255, alpha), width=1)
    rain.save(source_dir / "rain-layer.png", optimize=True)

    signal = Image.new("RGBA", (WIDTH, HEIGHT), (0, 0, 0, 0))
    signal_draw = ImageDraw.Draw(signal)
    points: list[tuple[int, int]] = []
    for x in range(0, WIDTH + 1, 4):
        envelope = math.exp(-((x - WIDTH * 0.58) / (WIDTH * 0.16)) ** 2)
        y = HEIGHT * 0.46 + envelope * (28 * math.sin(x * 0.065) + 9 * math.sin(x * 0.19))
        points.append((x, round(y)))
    signal_draw.line(points, fill=(255, 48, 176, 210), width=3)
    signal = signal.filter(ImageFilter.GaussianBlur(0.6))
    signal.save(source_dir / "signal-layer.png", optimize=True)
    return base, streaks


def _frame(base: Image.Image, streaks: list[tuple[int, int, int, int]], index: int) -> Image.Image:
    phase = index / FRAME_COUNT
    frame = base.convert("RGBA")
    rain = Image.new("RGBA", frame.size, (0, 0, 0, 0))
    rain_draw = ImageDraw.Draw(rain)
    offset = int(phase * HEIGHT * 3) % HEIGHT
    for x, y, length, alpha in streaks:
        shifted = (y + offset) % HEIGHT
        rain_draw.line((x, shifted, x - 5, shifted + length), fill=(145, 211, 255, alpha), width=1)
    frame.alpha_composite(rain)

    pulse = Image.new("RGBA", frame.size, (0, 0, 0, 0))
    pulse_draw = ImageDraw.Draw(pulse)
    center = int((0.08 + 0.84 * phase) * WIDTH)
    glow = 0.45 + 0.55 * math.sin(math.pi * phase) ** 2
    points: list[tuple[int, int]] = []
    for x in range(max(0, center - 430), min(WIDTH, center + 430), 3):
        envelope = math.exp(-((x - center) / 175) ** 2)
        y = HEIGHT * 0.46 + envelope * (24 * math.sin(x * 0.075) + 7 * math.sin(x * 0.21))
        points.append((x, round(y)))
    if len(points) > 1:
        pulse_draw.line(points, fill=(255, 42, 174, round(210 * glow)), width=3)
        pulse = pulse.filter(ImageFilter.GaussianBlur(0.7))
        frame.alpha_composite(pulse)
    return frame.convert("RGB")


def _encode_mp4(base: Image.Image, streaks: list[tuple[int, int, int, int]], output: Path) -> None:
    command = [
        "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
        "-f", "rawvideo", "-pixel_format", "rgb24", "-video_size", f"{WIDTH}x{HEIGHT}",
        "-framerate", str(FPS), "-i", "-", "-an", "-c:v", "libx264", "-preset", "slow",
        "-crf", "18", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(output),
    ]
    process = subprocess.Popen(command, stdin=subprocess.PIPE)
    assert process.stdin is not None
    try:
        for index in range(FRAME_COUNT):
            process.stdin.write(_frame(base, streaks, index).tobytes())
    except BrokenPipeError as error:
        raise RuntimeError("NATIVEVN_VIDEO_FFMPEG_ENCODE_FAILED") from error
    finally:
        process.stdin.close()
    if process.wait() != 0:
        raise RuntimeError("NATIVEVN_VIDEO_FFMPEG_ENCODE_FAILED")


def build(background: Path, output_dir: Path) -> None:
    if shutil.which("ffmpeg") is None or shutil.which("ffprobe") is None:
        raise RuntimeError("NATIVEVN_VIDEO_FFMPEG_MISSING")
    if not background.is_file():
        raise RuntimeError("NATIVEVN_VIDEO_BACKGROUND_MISSING")
    output_dir.mkdir(parents=True, exist_ok=True)
    source_dir = output_dir / "Source"
    base, streaks = _write_layers(source_dir, background)
    mp4 = output_dir / "rain-signal-loop.mp4"
    webm = output_dir / "rain-signal-loop.webm"
    _encode_mp4(base, streaks, mp4)
    subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(mp4), "-an",
         "-c:v", "libvpx-vp9", "-crf", "28", "-b:v", "0", "-pix_fmt", "yuv420p", str(webm)],
        check=True,
    )
    recipe = {
        "schema": "nativevn.flagship_video_recipe.v1",
        "id": "rain-signal-loop",
        "width": WIDTH,
        "height": HEIGHT,
        "frame_rate": FPS,
        "frame_count": FRAME_COUNT,
        "duration_ms": DURATION_SECONDS * 1000,
        "seed": SEED,
        "layers": ["base.png", "rain-layer.png", "signal-layer.png"],
        "outputs": {
            "mp4": {"path": "../rain-signal-loop.mp4", "sha256": _sha256(mp4)},
            "webm": {"path": "../rain-signal-loop.webm", "sha256": _sha256(webm)},
        },
    }
    (source_dir / "rebuild.json").write_text(json.dumps(recipe, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    repo = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--background", type=Path, default=repo / "Examples/NativeVN/Visual/KeyArt/title-background.png")
    parser.add_argument("--output-dir", type=Path, default=repo / "Examples/NativeVN/Visual/Video")
    args = parser.parse_args(argv)
    try:
        build(args.background, args.output_dir)
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(str(error), file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
