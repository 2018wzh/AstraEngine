#!/usr/bin/env python3
"""Rebuild the deterministic public-audio manifest from generated artifacts."""

from __future__ import annotations

import argparse
import sys
import wave
from pathlib import Path

from audio_catalog import AUDIO_SPECS
from common import Diagnostics, ToolFailure, display_path, sha256_file, write_json_atomic


MANIFEST_NAME = "audio-manifest.json"


def inspect_wav(path: Path) -> dict[str, int | float]:
    try:
        with wave.open(str(path), "rb") as stream:
            channels = stream.getnchannels()
            sample_width = stream.getsampwidth()
            sample_rate = stream.getframerate()
            frame_count = stream.getnframes()
            compression = stream.getcomptype()
    except (OSError, EOFError, wave.Error) as error:
        raise ToolFailure("NATIVEVN_WAV_INVALID", "WAV master is unreadable", path=path.name) from error
    if compression != "NONE":
        raise ToolFailure("NATIVEVN_WAV_CODEC_INVALID", "WAV master must use uncompressed PCM", path=path.name)
    return {
        "channels": channels,
        "sample_width_bits": sample_width * 8,
        "sample_rate": sample_rate,
        "frame_count": frame_count,
        "duration_seconds": round(frame_count / sample_rate, 6),
    }


def update_audio_manifest(audio_root: Path, *, require_complete: bool = True) -> dict[str, object]:
    audio_root = audio_root.resolve()
    assets: list[dict[str, object]] = []
    for spec in AUDIO_SPECS:
        master = audio_root / "Master" / spec.kind / f"{spec.asset_id}.wav"
        distribution = audio_root / "Distribution" / spec.kind / f"{spec.asset_id}.ogg"
        if not master.is_file() or not distribution.is_file():
            if require_complete:
                raise ToolFailure(
                    "NATIVEVN_AUDIO_ASSET_MISSING",
                    "catalog asset is missing its WAV master or OGG distribution",
                    path=f"{spec.kind}/{spec.asset_id}",
                )
            continue
        properties = inspect_wav(master)
        assets.append(
            {
                "id": spec.asset_id,
                "kind": spec.kind,
                "title": {"en": spec.title_en, "zh": spec.title_zh},
                "loop": spec.loop,
                "duration_seconds": properties["duration_seconds"],
                "master": {
                    "path": display_path(master, audio_root),
                    "sha256": sha256_file(master),
                    "byte_size": master.stat().st_size,
                    **{key: value for key, value in properties.items() if key != "duration_seconds"},
                },
                "distribution": {
                    "path": display_path(distribution, audio_root),
                    "format": "ogg_vorbis",
                    "sha256": sha256_file(distribution),
                    "byte_size": distribution.stat().st_size,
                },
                "provenance": {
                    "generator": "Tools/NativeVN/generate_audio.py",
                    "recipe": spec.synthesis,
                    "seed": spec.seed,
                    "original": True,
                },
                "release_eligible": True,
                "license_status": "original_project_asset",
            }
        )
    payload: dict[str, object] = {
        "schema": "astra.nativevn_flagship.audio_manifest.v1",
        "id": "nativevn-flagship-audio",
        "sample_contract": {"sample_rate": 48_000, "sample_width_bits": 24, "channels": 2},
        "asset_count": len(assets),
        "assets": assets,
    }
    write_json_atomic(audio_root / MANIFEST_NAME, payload)
    return payload


def _default_audio_root() -> Path:
    return Path(__file__).resolve().parents[2] / "Examples" / "NativeVN" / "Audio"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--audio-root", type=Path, default=_default_audio_root())
    parser.add_argument("--allow-incomplete", action="store_true", help="write a development manifest for present assets only")
    args = parser.parse_args(argv)
    diagnostics = Diagnostics()
    try:
        manifest = update_audio_manifest(args.audio_root, require_complete=not args.allow_incomplete)
    except ToolFailure as error:
        diagnostics.error(error.code, error.message, error.path)
        diagnostics.emit_json()
        return 2
    except OSError:
        diagnostics.error("NATIVEVN_MANIFEST_IO_FAILED", "manifest update failed during a filesystem operation")
        diagnostics.emit_json()
        return 2
    diagnostics.emit_json(summary={"asset_count": manifest["asset_count"], "manifest": MANIFEST_NAME})
    return 0


if __name__ == "__main__":
    sys.exit(main())
