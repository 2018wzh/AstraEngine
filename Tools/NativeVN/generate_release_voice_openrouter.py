#!/usr/bin/env python3
"""Generate the authorized NativeVN release voice set through OpenRouter."""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


MODEL = "x-ai/grok-voice-tts-1.0"
VOICE_BY_SPEAKER = {"lin_yao": "Eve", "zhou_heng": "Rex"}
STYLE_BY_ROUTE = {
    "common": "[calm][measured]",
    "truth": "[focused][urgent]",
    "silence": "[calm][subdued]",
    "signal": "[warm][measured]",
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def load_lines(pack: Path) -> dict[str, dict[str, Any]]:
    screenplay = json.loads((pack / "Narrative" / "screenplay.zh-Hans.json").read_text(encoding="utf-8"))
    result: dict[str, dict[str, Any]] = {}
    for scene in screenplay["scenes"]:
        for line in scene["lines"]:
            if line["id"] in result:
                raise ValueError(f"duplicate screenplay line: {line['id']}")
            result[line["id"]] = line
    return result


def load_cues(pack: Path) -> list[dict[str, Any]]:
    payload = json.loads((pack / "Narrative" / "voice-cues.json").read_text(encoding="utf-8"))
    lines = load_lines(pack)
    cues = []
    seen = set()
    for cue in payload["cues"]:
        if cue["id"] in seen or cue["line_id"] not in lines:
            raise ValueError(f"invalid voice cue binding: {cue['id']}")
        seen.add(cue["id"])
        line = lines[cue["line_id"]]
        if line["voice_cue_id"] != cue["id"] or line["speaker"] != cue["speaker_id"]:
            raise ValueError(f"voice cue does not match canonical line: {cue['id']}")
        cues.append({**cue, "text": line["text"]})
    if len(cues) != 180 or len(lines) != 180:
        raise ValueError("release voice generation requires exactly 180 canonical cues")
    return cues


def synthesize(api_key: str, cue: dict[str, Any], source_root: Path, timeout: float) -> tuple[dict[str, Any], Path, str]:
    speaker = cue["speaker_id"]
    voice = VOICE_BY_SPEAKER[speaker]
    prompt = f"{STYLE_BY_ROUTE[cue['route_scope']]} {cue['text']}"
    destination = source_root / speaker / f"{cue['id']}.mp3"
    destination.parent.mkdir(parents=True, exist_ok=True)
    request_hash = hashlib.sha256(prompt.encode("utf-8")).hexdigest()
    if destination.is_file() and destination.stat().st_size >= 1024:
        return cue, destination, request_hash
    body = json.dumps({
        "model": MODEL,
        "input": prompt,
        "voice": voice,
        "response_format": "mp3",
        "speed": 1.0,
    }, ensure_ascii=False).encode("utf-8")
    request = urllib.request.Request(
        "https://openrouter.ai/api/v1/audio/speech",
        data=body,
        method="POST",
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
            "X-Title": "AstraEngine NativeVN",
        },
    )
    for attempt in range(5):
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                audio = response.read()
            if len(audio) < 1024:
                raise RuntimeError("OpenRouter returned an undersized voice response")
            temporary = destination.with_suffix(".tmp")
            temporary.write_bytes(audio)
            os.replace(temporary, destination)
            return cue, destination, request_hash
        except urllib.error.HTTPError as error:
            if error.code not in {408, 409, 429, 500, 502, 503, 504} or attempt == 4:
                detail = error.read().decode("utf-8", "replace")[:500]
                raise RuntimeError(f"OpenRouter rejected {cue['id']} with HTTP {error.code}: {detail}") from error
        except (urllib.error.URLError, TimeoutError) as error:
            if attempt == 4:
                raise RuntimeError(f"OpenRouter unavailable for {cue['id']}") from error
        time.sleep(2**attempt)
    raise AssertionError("unreachable retry state")


def transcode(source: Path, master: Path, distribution: Path) -> None:
    master.parent.mkdir(parents=True, exist_ok=True)
    distribution.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run([
        "ffmpeg", "-v", "error", "-y", "-i", str(source),
        "-af", "adelay=120,apad=pad_dur=0.22,loudnorm=I=-19:TP=-3:LRA=7",
        "-ar", "48000", "-ac", "1", "-c:a", "pcm_s24le", str(master),
    ], check=True)
    subprocess.run([
        "ffmpeg", "-v", "error", "-y", "-i", str(master),
        "-c:a", "libvorbis", "-q:a", "6", str(distribution),
    ], check=True)


def probe(path: Path) -> dict[str, Any]:
    completed = subprocess.run([
        "ffprobe", "-v", "error", "-select_streams", "a:0",
        "-show_entries", "stream=codec_name,sample_rate,channels,bits_per_raw_sample:format=duration",
        "-of", "json", str(path),
    ], check=True, capture_output=True, text=True)
    payload = json.loads(completed.stdout)
    stream = payload["streams"][0]
    return {
        "codec": stream["codec_name"],
        "sample_rate_hz": int(stream["sample_rate"]),
        "channels": int(stream["channels"]),
        "duration_ms": round(float(payload["format"]["duration"]) * 1000),
    }


def generate(pack: Path, private_root: Path, workers: int, timeout: float) -> dict[str, Any]:
    api_key = os.environ.get("OPENROUTER_API_KEY", "").strip()
    if not api_key:
        raise RuntimeError("OPENROUTER_API_KEY is required")
    cues = load_cues(pack)
    source_root = private_root / "openrouter-voice-source"
    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as executor:
        futures = [executor.submit(synthesize, api_key, cue, source_root, timeout) for cue in cues]
        for index, future in enumerate(concurrent.futures.as_completed(futures), 1):
            results.append(future.result())
            print(json.dumps({"event": "voice.generated", "completed": index, "total": len(cues)}), flush=True)
    results.sort(key=lambda item: item[0]["id"])
    records = []
    for cue, source, request_hash in results:
        speaker = cue["speaker_id"]
        master = pack / "Audio" / "Voice" / "Master" / speaker / f"{cue['id']}.wav"
        distribution = pack / "Audio" / "Voice" / "Distribution" / speaker / f"{cue['id']}.ogg"
        transcode(source, master, distribution)
        source_media = probe(source)
        master_media = probe(master)
        distribution_media = probe(distribution)
        if master_media["sample_rate_hz"] != 48000 or master_media["channels"] != 1 or distribution_media["sample_rate_hz"] != 48000:
            raise RuntimeError(f"voice media contract failed for {cue['id']}")
        records.append({
            "id": cue["id"],
            "role": "dialogue",
            "scene_id": cue["scene_id"],
            "route_id": f"route_{cue['route_scope']}",
            "line_id": cue["line_id"],
            "speaker_id": speaker,
            "take": 1,
            "asset": {"relative_path": distribution.relative_to(pack).as_posix()},
            "master": {"relative_path": master.relative_to(pack).as_posix()},
            "sha256": sha256(distribution),
            "byte_size": distribution.stat().st_size,
            "media": {"mime_type": "audio/ogg", **distribution_media},
            "generation_source": {
                "kind": "ai_assisted",
                "method": "text-to-speech",
                "tool": "openrouter",
                "model_id": MODEL,
                "voice_id": VOICE_BY_SPEAKER[speaker],
                "request_text_sha256": request_hash,
                "source_audio_sha256": sha256(source),
                "source_media": source_media,
                "master_sha256": sha256(master),
            },
            "license_status": "user_authorized",
            "license_evidence": {"relative_path": "STATUS.md"},
            "release_eligible": True,
        })
    manifest = {
        "schema": "nativevn.flagship_voice_cues.v1",
        "package_id": "com.astra.nativevn.signal-glass-rain",
        "status": "release_ready",
        "model": MODEL,
        "voices": VOICE_BY_SPEAKER,
        "cue_count": len(records),
        "cues": records,
    }
    destination = pack / "Manifests" / "voice-release.json"
    destination.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return manifest


def main() -> int:
    repo = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pack-root", type=Path, default=repo / "Examples" / "NativeVN")
    parser.add_argument("--private-root", type=Path, default=repo / ".tmp")
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--timeout", type=float, default=120.0)
    args = parser.parse_args()
    if not 1 <= args.workers <= 8:
        raise SystemExit("workers must be between 1 and 8")
    manifest = generate(args.pack_root.resolve(), args.private_root.resolve(), args.workers, args.timeout)
    print(json.dumps({"status": "pass", "cue_count": manifest["cue_count"], "model": manifest["model"]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
