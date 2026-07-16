#!/usr/bin/env python3
"""Measure loudness, true peak, spectrum activity, clipping, silence, and loop seams."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import wave
from pathlib import Path

from common import Diagnostics, ToolFailure, display_path, sha256_file, write_json_atomic
from validate_content_pack import inspect_wav


LOUDNESS_JSON = re.compile(r"\{\s*\"input_i\".*?\}", re.DOTALL)
CENTROID = re.compile(r"lavfi\.aspectralstats\.1\.centroid=([0-9.eE+-]+)")


def _decode_pcm24(payload: bytes) -> list[float]:
    values: list[float] = []
    for offset in range(0, len(payload) - 2, 3):
        value = payload[offset] | payload[offset + 1] << 8 | payload[offset + 2] << 16
        if value & 0x800000:
            value -= 1 << 24
        values.append(value / 8_388_608.0)
    return values


def loop_seam_delta(path: Path) -> float:
    with wave.open(str(path), "rb") as stream:
        channels = stream.getnchannels()
        first = _decode_pcm24(stream.readframes(2))
        stream.setpos(max(0, stream.getnframes() - 2))
        last = _decode_pcm24(stream.readframes(2))
    if len(first) < 2 * channels or len(last) < 2 * channels:
        raise ToolFailure("NATIVEVN_AUDIO_SEAM_UNREADABLE", "loop seam window is empty", path=path.name)
    boundary = max(abs(last[channels + channel] - first[channel]) for channel in range(channels))
    slope = max(
        abs((first[channels + channel] - first[channel]) - (last[channels + channel] - last[channel]))
        for channel in range(channels)
    )
    return max(boundary, slope)


def ffmpeg_metrics(path: Path) -> tuple[float, float, float, float, float]:
    loudness = subprocess.run(
        ["ffmpeg", "-hide_banner", "-nostats", "-i", str(path), "-af", "loudnorm=I=-18:TP=-2:LRA=11:print_format=json", "-f", "null", "-"],
        capture_output=True, text=True, encoding="utf-8", errors="replace", check=False,
    )
    match = LOUDNESS_JSON.search(loudness.stderr)
    if loudness.returncode != 0 or match is None:
        raise ToolFailure("NATIVEVN_AUDIO_LOUDNESS_FAILED", "ffmpeg loudness analysis failed", path=path.name)
    payload = json.loads(match.group(0))

    spectrum = subprocess.run(
        ["ffmpeg", "-hide_banner", "-nostats", "-t", "5", "-i", str(path), "-af", "aspectralstats=measure=centroid,ametadata=print:key=lavfi.aspectralstats.1.centroid", "-f", "null", "-"],
        capture_output=True, text=True, encoding="utf-8", errors="replace", check=False,
    )
    centroids = [float(value) for value in CENTROID.findall(spectrum.stderr)]
    active_centroids = [value for value in centroids if value >= 20.0]
    if spectrum.returncode != 0 or not active_centroids:
        raise ToolFailure("NATIVEVN_AUDIO_SPECTRUM_FAILED", "ffmpeg spectrum analysis failed", path=path.name)
    return float(payload["input_i"]), float(payload["input_tp"]), float(payload["input_lra"]), min(active_centroids), max(active_centroids)


def analyze(audio_root: Path) -> dict[str, object]:
    manifest_path = audio_root / "audio-manifest.json"
    if not manifest_path.is_file():
        raise ToolFailure("NATIVEVN_AUDIO_MANIFEST_MISSING", "audio manifest is required before QA")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    results: list[dict[str, object]] = []
    for asset in manifest.get("assets", []):
        master = audio_root / asset["master"]["path"]
        info = inspect_wav(master)
        integrated, true_peak, loudness_range, centroid_min, centroid_max = ffmpeg_metrics(master)
        seam = loop_seam_delta(master) if asset.get("loop") else None
        finding_codes: list[str] = []
        if info.clipped_samples:
            finding_codes.append("clipping")
        if info.rms_dbfs < -58.0 or info.peak < 0.001:
            finding_codes.append("silence")
        if true_peak > -1.0:
            finding_codes.append("true_peak")
        if seam is not None and seam > 0.05:
            finding_codes.append("loop_seam")
        if not 20.0 <= centroid_min <= centroid_max <= 24_000.0:
            finding_codes.append("spectrum")
        results.append({
            "id": asset["id"],
            "kind": asset["kind"],
            "master_path": display_path(master, audio_root),
            "sha256": sha256_file(master),
            "duration_seconds": round(info.duration_seconds, 6),
            "integrated_loudness_lufs": integrated,
            "true_peak_dbtp": true_peak,
            "loudness_range_lu": loudness_range,
            "sample_peak": round(info.peak, 8),
            "rms_dbfs": round(info.rms_dbfs, 4),
            "clipped_samples": info.clipped_samples,
            "spectral_centroid_hz": {"minimum": round(centroid_min, 3), "maximum": round(centroid_max, 3)},
            "loop_seam_max_delta": None if seam is None else round(seam, 8),
            "automated_decision": "pass" if not finding_codes else "blocked",
            "finding_codes": finding_codes,
        })
    payload: dict[str, object] = {
        "schema": "astra.nativevn_flagship.audio_qa.v1",
        "id": "nativevn-flagship-audio-qa",
        "analysis_tool": "ffmpeg-loudnorm-aspectralstats",
        "asset_count": len(results),
        "automated_decision": "pass" if results and all(item["automated_decision"] == "pass" for item in results) else "blocked",
        "manual_listening": {"status": "not_performed", "required_before_release": True},
        "assets": results,
    }
    write_json_atomic(audio_root / "audio-qa-report.json", payload)
    return payload


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--audio-root", type=Path, default=Path(__file__).resolve().parents[2] / "Examples" / "NativeVN" / "Audio")
    args = parser.parse_args(argv)
    diagnostics = Diagnostics()
    try:
        report = analyze(args.audio_root.resolve())
    except (OSError, ValueError, KeyError, json.JSONDecodeError, ToolFailure) as error:
        if isinstance(error, ToolFailure):
            diagnostics.error(error.code, error.message, error.path)
        else:
            diagnostics.error("NATIVEVN_AUDIO_QA_FAILED", "audio QA failed while reading or measuring an asset")
        diagnostics.emit_json()
        return 2
    if report["automated_decision"] != "pass":
        diagnostics.error("NATIVEVN_AUDIO_QA_BLOCKED", "one or more public audio assets failed automated QA")
        diagnostics.emit_json(summary={"asset_count": report["asset_count"]})
        return 2
    diagnostics.emit_json(summary={"asset_count": report["asset_count"], "manual_listening": "not_performed"})
    return 0


if __name__ == "__main__":
    sys.exit(main())
