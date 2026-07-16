#!/usr/bin/env python3
"""Generate deterministic, layered original music and sound effects."""

from __future__ import annotations

import argparse
import json
import math
import os
import subprocess
import sys
import wave
from dataclasses import dataclass
from pathlib import Path

from audio_catalog import AUDIO_SPECS, AudioSpec
from common import Diagnostics, ToolFailure, require_executable, require_within


SAMPLE_RATE = 48_000
SAMPLE_WIDTH = 3
CHANNELS = 2
TAU = math.tau


@dataclass
class FilterState:
    low_l: float = 0.0
    low_r: float = 0.0
    delay_l: list[float] | None = None
    delay_r: list[float] | None = None
    delay_index: int = 0


def _fract(value: float) -> float:
    return value - math.floor(value)


def _hash_noise(index: int, seed: int) -> float:
    value = (index * 0x45D9F3B + seed * 0x27D4EB2D) & 0xFFFFFFFF
    value = ((value >> 16) ^ value) * 0x45D9F3B & 0xFFFFFFFF
    value = ((value >> 16) ^ value) * 0x45D9F3B & 0xFFFFFFFF
    value = (value >> 16) ^ value
    return (value / 0x7FFFFFFF) - 1.0


def _smooth_noise(t: float, rate: float, seed: int) -> float:
    position = t * rate
    left = math.floor(position)
    mix = _fract(position)
    mix = mix * mix * (3.0 - 2.0 * mix)
    return _hash_noise(left, seed) * (1.0 - mix) + _hash_noise(left + 1, seed) * mix


def _periodic_noise(t: float, duration: float, seed: int, layers: int = 9) -> float:
    value = 0.0
    weight = 0.0
    for layer in range(1, layers + 1):
        harmonic = 5 + ((seed + layer * 37) % 181)
        amplitude = 1.0 / math.sqrt(layer)
        phase = ((seed * (layer + 3)) % 997) / 997.0
        value += math.sin(TAU * (harmonic * t / duration + phase)) * amplitude
        weight += amplitude
    return value / weight


def _quantized_frequency(frequency: float, duration: float) -> float:
    return max(1, round(frequency * duration)) / duration


def _osc(t: float, frequency: float, phase: float = 0.0) -> float:
    return math.sin(TAU * (frequency * t + phase))


def _triangle(t: float, frequency: float) -> float:
    return 2.0 * abs(2.0 * _fract(t * frequency + 0.25) - 1.0) - 1.0


def _attack_release(t: float, duration: float, attack: float, release: float) -> float:
    attack_gain = min(1.0, max(0.0, t / max(attack, 1e-6)))
    release_gain = min(1.0, max(0.0, (duration - t) / max(release, 1e-6)))
    return math.sin(attack_gain * math.pi / 2.0) * math.sin(release_gain * math.pi / 2.0)


def _pulse(t: float, at: float, decay: float) -> float:
    elapsed = t - at
    return 0.0 if elapsed < 0.0 else math.exp(-elapsed / decay)


def _bgm_sample(spec: AudioSpec, t: float) -> tuple[float, float]:
    duration = spec.duration_seconds
    phase = t / duration
    root = {"rain_glass": 110.0, "carrier": 73.42, "thin_line": 146.83, "after_signal": 98.0}[spec.synthesis]
    root = _quantized_frequency(root, duration)
    fifth = _quantized_frequency(root * 1.5, duration)
    octave = _quantized_frequency(root * 2.0, duration)
    slow = 0.5 - 0.5 * math.cos(TAU * phase)
    motif_gate = 0.55 + 0.45 * math.sin(TAU * (8.0 * phase)) ** 2
    drone = 0.22 * _osc(t, root) + 0.12 * _osc(t, fifth, 0.17) + 0.07 * _osc(t, _quantized_frequency(root / 2.0, duration), 0.31)
    shimmer = 0.08 * _osc(t, octave * 2.0 + 7.0 / duration, 0.4) * motif_gate
    texture = _periodic_noise(t, duration, spec.seed, 11)
    if spec.synthesis == "rain_glass":
        bells = sum(
            _osc(t, _quantized_frequency(root * ratio, duration), index * 0.13)
            for index, ratio in enumerate((2.0, 2.5, 3.0, 4.25))
        ) * (0.035 + 0.045 * slow)
        center = drone + bells + shimmer + texture * 0.12
    elif spec.synthesis == "carrier":
        carrier = _osc(t, _quantized_frequency(880.0, duration), 0.1) * _osc(t, 13.0 / duration)
        sideband = _osc(t, _quantized_frequency(1760.0, duration), 0.25) * _osc(t, 29.0 / duration)
        center = drone * 0.65 + carrier * 0.055 + sideband * 0.035 + texture * 0.22
    elif spec.synthesis == "thin_line":
        pluck = _triangle(t, _quantized_frequency(root * 2.0, duration))
        sequence = 0.5 + 0.5 * math.sin(TAU * 12.0 * phase)
        center = drone * 0.75 + pluck * sequence * 0.075 + shimmer * 1.3 + texture * 0.08
    else:
        answer = _osc(t, _quantized_frequency(root * 2.25, duration), 0.28) * (0.3 + 0.7 * slow)
        center = drone * 0.9 + answer * 0.08 + shimmer + texture * 0.14
    width = 0.07 * _periodic_noise(t, duration, spec.seed + 101, 7)
    pan = math.sin(TAU * (3.0 * phase + (spec.seed % 17) / 17.0))
    return center * (1.0 - 0.12 * pan) + width, center * (1.0 + 0.12 * pan) - width


def _event_sample(spec: AudioSpec, t: float) -> tuple[float, float]:
    duration = spec.duration_seconds
    env = _attack_release(t, duration, min(0.12, duration * 0.08), min(1.8, duration * 0.35))
    noise = _smooth_noise(t, 9_000.0, spec.seed)
    low_noise = _smooth_noise(t, 180.0, spec.seed + 31)
    name = spec.synthesis
    left = right = 0.0
    if name in {"truth", "silence", "signal"}:
        roots = {"truth": 130.81, "silence": 87.31, "signal": 164.81}
        root = roots[name]
        chord = _osc(t, root) * 0.30 + _osc(t, root * 1.5, 0.12) * 0.19 + _osc(t, root * 2.0, 0.3) * 0.12
        rise = _osc(t, root * (2.0 + 0.75 * t / duration), 0.2) * 0.13
        accent = _pulse(t, duration * 0.58, 0.42) * (_osc(t, root * 4.0) * 0.18 + noise * 0.08)
        if name == "silence":
            chord *= max(0.0, 1.0 - t / (duration * 0.72))
            rise *= 0.35
        left = env * (chord + rise + accent + low_noise * 0.045)
        right = env * (chord - rise * 0.45 + accent * 0.8 - low_noise * 0.045)
    elif name in {"rain_window", "radio_static", "vent", "electrical_hum"}:
        base = noise * {"rain_window": 0.35, "radio_static": 0.42, "vent": 0.22, "electrical_hum": 0.04}[name]
        if name == "rain_window":
            drops = sum(_pulse(t, duration * fraction, 0.035) for fraction in (0.13, 0.29, 0.47, 0.71, 0.88))
            base += drops * (_osc(t, 1550.0) + noise * 0.4) * 0.13
        elif name == "radio_static":
            base *= 0.6 + 0.4 * (_osc(t, 6.7) ** 2)
            base += _osc(t, 1210.0 + 35.0 * _osc(t, 1.7)) * 0.09
        elif name == "vent":
            base += low_noise * 0.28 + _osc(t, 58.0) * 0.06
        else:
            base += _osc(t, 60.0) * 0.17 + _osc(t, 120.0) * 0.08 + _osc(t, 720.0) * 0.025
        left = env * (base + low_noise * 0.06)
        right = env * (base - low_noise * 0.06)
    elif name == "thunder":
        boom = (_osc(t, 43.0) * 0.5 + _osc(t, 61.0) * 0.3 + low_noise * 0.5) * _pulse(t, 0.35, 1.4)
        crack = noise * _pulse(t, 0.28, 0.16) * 0.35
        left, right = env * (boom + crack), env * (boom * 0.92 - crack * 0.4)
    elif name in {"signal_lock", "terminal_wake", "terminal_key", "terminal_confirm", "terminal_deny", "message_arrive", "choice_open", "choice_select"}:
        tones = {
            "signal_lock": (440.0, 880.0), "terminal_wake": (110.0, 660.0), "terminal_key": (780.0, 1170.0),
            "terminal_confirm": (660.0, 990.0), "terminal_deny": (330.0, 247.0), "message_arrive": (523.25, 784.88),
            "choice_open": (392.0, 587.33), "choice_select": (587.33, 880.0),
        }
        first, second = tones[name]
        sweep = first + (second - first) * min(1.0, t / max(0.08, duration * 0.55))
        body = _osc(t, sweep) * 0.30 + _osc(t, sweep * 2.01, 0.2) * 0.10 + noise * 0.035
        clicks = _pulse(t, 0.0, 0.025) + 0.7 * _pulse(t, duration * 0.48, 0.04)
        body += clicks * (_osc(t, second * 1.5) + noise * 0.3) * 0.12
        left, right = env * body, env * (body * 0.91 + _osc(t, sweep * 0.997, 0.25) * 0.035)
    elif name in {"archive_open", "door_pressure", "footstep_metal", "glass_fracture", "memory_flare", "scene_transition"}:
        if name == "archive_open":
            body = low_noise * 0.18 + _osc(t, 82.0 + 45.0 * t) * 0.14 + noise * _pulse(t, 0.15, 0.5) * 0.12
        elif name == "door_pressure":
            body = low_noise * 0.33 + _osc(t, 48.0) * 0.18 + noise * (_pulse(t, 0.2, 0.45) + _pulse(t, 2.2, 0.25)) * 0.18
        elif name == "footstep_metal":
            body = _osc(t, 92.0) * _pulse(t, 0.02, 0.14) * 0.42 + noise * _pulse(t, 0.0, 0.07) * 0.30
        elif name == "glass_fracture":
            shards = sum(_osc(t, frequency) for frequency in (1319.0, 1763.0, 2349.0, 3011.0)) * 0.07
            body = (shards + noise * 0.33) * (_pulse(t, 0.08, 0.42) + 0.5 * _pulse(t, 0.7, 0.3))
        elif name == "memory_flare":
            sweep = 180.0 * (1.0 + 5.0 * t / duration)
            body = _osc(t, sweep) * 0.23 + _osc(t, sweep * 1.503, 0.1) * 0.13 + noise * 0.08
        else:
            sweep = 90.0 + 720.0 * (t / duration) ** 2
            body = _osc(t, sweep) * 0.22 + low_noise * 0.12 + noise * 0.05
        left, right = env * body, env * (body * 0.86 - low_noise * 0.05)
    else:
        raise ValueError(f"unsupported synthesis recipe: {name}")
    return left, right


def _pack_sample(value: float, output: bytearray) -> None:
    softened = math.tanh(value * 1.25) * 0.72
    integer = max(-8_388_608, min(8_388_607, round(softened * 8_388_607)))
    unsigned = integer & 0xFFFFFF
    output.extend((unsigned & 0xFF, (unsigned >> 8) & 0xFF, (unsigned >> 16) & 0xFF))


def render_wav(spec: AudioSpec, destination: Path, *, sample_rate: int = SAMPLE_RATE) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(f".{destination.name}.tmp")
    frame_count = round(spec.duration_seconds * sample_rate)
    delay_frames = max(1, round(0.173 * sample_rate))
    state = FilterState(delay_l=[0.0] * delay_frames, delay_r=[0.0] * delay_frames)
    with wave.open(str(temporary), "wb") as output:
        output.setnchannels(CHANNELS)
        output.setsampwidth(SAMPLE_WIDTH)
        output.setframerate(sample_rate)
        for block_start in range(0, frame_count, 4096):
            frames = bytearray()
            for index in range(block_start, min(frame_count, block_start + 4096)):
                t = index / sample_rate
                left, right = _bgm_sample(spec, t) if spec.loop else _event_sample(spec, t)
                if spec.loop:
                    # Every BGM oscillator and modulation source completes an integer
                    # number of cycles. Avoid stateful startup filters here so the final
                    # frame joins the first without a reverb/filter-state discontinuity.
                    rendered_l, rendered_r = left, right
                else:
                    state.low_l += 0.16 * (left - state.low_l)
                    state.low_r += 0.16 * (right - state.low_r)
                    delayed_l = state.delay_l[state.delay_index]
                    delayed_r = state.delay_r[state.delay_index]
                    state.delay_l[state.delay_index] = state.low_l + delayed_r * 0.20
                    state.delay_r[state.delay_index] = state.low_r + delayed_l * 0.20
                    state.delay_index = (state.delay_index + 1) % delay_frames
                    rendered_l = state.low_l + delayed_l * 0.22
                    rendered_r = state.low_r + delayed_r * 0.22
                _pack_sample(rendered_l, frames)
                _pack_sample(rendered_r, frames)
            output.writeframesraw(frames)
    os.replace(temporary, destination)


def encode_ogg(ffmpeg: str, source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(f".{destination.stem}.tmp.ogg")
    command = [
        ffmpeg, "-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i", str(source),
        "-map_metadata", "-1", "-vn", "-c:a", "libvorbis", "-q:a", "6", "-ar", str(SAMPLE_RATE),
        "-ac", str(CHANNELS), str(temporary),
    ]
    completed = subprocess.run(command, capture_output=True, text=True, encoding="utf-8", errors="replace", check=False)
    if completed.returncode != 0 or not temporary.is_file() or temporary.stat().st_size == 0:
        temporary.unlink(missing_ok=True)
        raise ToolFailure("NATIVEVN_OGG_ENCODE_FAILED", "ffmpeg failed to encode an OGG distribution asset", path=source.name)
    os.replace(temporary, destination)


def require_vorbis_encoder() -> str:
    ffmpeg = require_executable("ffmpeg")
    completed = subprocess.run(
        [ffmpeg, "-hide_banner", "-encoders"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if completed.returncode != 0 or "libvorbis" not in completed.stdout:
        raise ToolFailure(
            "NATIVEVN_FFMPEG_VORBIS_MISSING",
            "ffmpeg is present but the required libvorbis encoder is unavailable",
        )
    return ffmpeg


def generate(output_root: Path, *, selected: set[str] | None = None, force: bool = False) -> list[AudioSpec]:
    output_root = output_root.resolve()
    ffmpeg = require_vorbis_encoder()
    specs = [spec for spec in AUDIO_SPECS if selected is None or spec.asset_id in selected]
    if selected is not None:
        unknown = selected.difference(spec.asset_id for spec in AUDIO_SPECS)
        if unknown:
            raise ToolFailure("NATIVEVN_AUDIO_ID_UNKNOWN", f"unknown audio id: {sorted(unknown)[0]}")
    generated: list[AudioSpec] = []
    for spec in specs:
        master = require_within(output_root / "Master" / spec.kind / f"{spec.asset_id}.wav", output_root)
        distribution = require_within(output_root / "Distribution" / spec.kind / f"{spec.asset_id}.ogg", output_root)
        master_action = "reused"
        distribution_action = "reused"
        if force or not master.exists():
            render_wav(spec, master)
            master_action = "rendered"
        if force or not distribution.exists() or distribution.stat().st_mtime_ns < master.stat().st_mtime_ns:
            encode_ogg(ffmpeg, master, distribution)
            distribution_action = "encoded"
        generated.append(spec)
        print(json.dumps({"event": "audio.ready", "id": spec.asset_id, "kind": spec.kind, "master": master_action, "distribution": distribution_action}, sort_keys=True))
    return generated


def _default_output() -> Path:
    return Path(__file__).resolve().parents[2] / "Examples" / "NativeVN" / "Audio"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=_default_output())
    parser.add_argument("--only", action="append", default=[], metavar="ASSET_ID")
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args(argv)
    diagnostics = Diagnostics()
    try:
        generated = generate(args.output, selected=set(args.only) or None, force=args.force)
        from update_manifest import update_audio_manifest

        update_audio_manifest(args.output)
    except ToolFailure as error:
        diagnostics.error(error.code, error.message, error.path)
        diagnostics.emit_json()
        return 2
    except OSError:
        diagnostics.error("NATIVEVN_AUDIO_IO_FAILED", "audio generation failed during a filesystem or process operation")
        diagnostics.emit_json()
        return 2
    diagnostics.emit_json(summary={"asset_count": len(generated), "sample_rate": SAMPLE_RATE, "sample_width_bits": 24})
    return 0


if __name__ == "__main__":
    sys.exit(main())
