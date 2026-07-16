#!/usr/bin/env python3
"""Generate private, noncommercial voice previews through ElevenLabs."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

from common import Diagnostics, ToolFailure, load_json, sha256_file, validate_safe_id, write_json_atomic


API_KEY_ENV = "ELEVENLABS_API_KEY"
VOICE_ID_ENV = {"lin_yao": "ELEVENLABS_LIN_YAO_VOICE_ID", "zhou_heng": "ELEVENLABS_ZHOU_HENG_VOICE_ID"}
DEFAULT_MODEL = "eleven_multilingual_v2"
VOICE_MANIFEST = "voice-manifest.json"
PUBLIC_METADATA = "voice-candidate-metadata.json"
FORBIDDEN_CLONE_FIELDS = {"clone", "cloned", "voice_clone", "sample_upload", "training_audio", "instant_voice_clone"}


def _default_pack_root() -> Path:
    return Path(__file__).resolve().parents[2] / "Examples" / "NativeVN"


def _validate_cues(payload: Any, *, line_texts: dict[str, str] | None = None) -> list[dict[str, Any]]:
    accepted_schemas = {
        "astra.nativevn_flagship.voice_cues.v1",
        "astra.nativevn.flagship.voice_cues.v1",
        "nativevn.flagship_voice_cues.v1",
    }
    if not isinstance(payload, dict) or payload.get("schema") not in accepted_schemas:
        raise ToolFailure("NATIVEVN_VOICE_CUES_SCHEMA_INVALID", "voice cues must declare a supported NativeVN flagship voice-cues v1 schema")
    cues = payload.get("cues")
    if not isinstance(cues, list) or not cues:
        raise ToolFailure("NATIVEVN_VOICE_CUES_EMPTY", "voice cues must contain a non-empty cues array")
    seen: set[str] = set()
    validated: list[dict[str, Any]] = []
    actor_profiles = {
        actor["speaker_id"]: actor
        for actor in payload.get("actors", [])
        if isinstance(actor, dict) and isinstance(actor.get("speaker_id"), str)
    } if isinstance(payload.get("actors"), list) else {}
    for cue in cues:
        if not isinstance(cue, dict):
            raise ToolFailure("NATIVEVN_VOICE_CUE_INVALID", "each voice cue must be an object")
        forbidden = FORBIDDEN_CLONE_FIELDS.intersection(cue)
        if forbidden:
            raise ToolFailure("NATIVEVN_VOICE_CLONING_FORBIDDEN", "voice cloning fields are forbidden")
        cue_id = validate_safe_id(cue.get("id"), "voice cue id")
        if cue_id in seen:
            raise ToolFailure("NATIVEVN_VOICE_CUE_DUPLICATE", "voice cue identifiers must be unique")
        seen.add(cue_id)
        speaker_id = cue.get("speaker_id") or cue.get("speaker")
        profile = payload.get("voice_profiles", {}).get(speaker_id, {}) if isinstance(payload.get("voice_profiles"), dict) else {}
        if not profile:
            profile = actor_profiles.get(speaker_id, {})
        if isinstance(profile, str):
            profile = {"voice_id": profile, "voice_source": "elevenlabs_library"}
        if not isinstance(profile, dict):
            profile = {}
        voice_id = cue.get("voice_id") or profile.get("elevenlabs_voice_id") or profile.get("voice_id")
        if not isinstance(voice_id, str) or not 6 <= len(voice_id) <= 64 or not voice_id.replace("-", "").replace("_", "").isalnum():
            raise ToolFailure("NATIVEVN_VOICE_ID_INVALID", "voice_id must reference an existing ElevenLabs library voice")
        voice_source = cue.get("voice_source", profile.get("voice_source", "elevenlabs_library"))
        if voice_source not in {"elevenlabs_library", "elevenlabs_default", "elevenlabs_voice_design"}:
            raise ToolFailure("NATIVEVN_VOICE_CLONING_FORBIDDEN", "only ElevenLabs Voice Design, library, or default voices are allowed")
        text = cue.get("text")
        if isinstance(text, dict):
            locale = cue.get("locale", "zh")
            text = text.get(locale)
        if text is None and isinstance(cue.get("line_id"), str) and line_texts is not None:
            text = line_texts.get(cue["line_id"])
        if not isinstance(text, str) or not text.strip() or len(text) > 5_000:
            raise ToolFailure("NATIVEVN_VOICE_TEXT_INVALID", "voice cue text must be non-empty and at most 5000 characters")
        validated.append({**cue, "id": cue_id, "voice_id": voice_id, "resolved_text": text.strip()})
    return validated


def _load_line_texts(pack_root: Path) -> dict[str, str]:
    screenplay = pack_root / "Narrative" / "screenplay.zh-Hans.json"
    if not screenplay.is_file():
        return {}
    payload = load_json(screenplay)
    if not isinstance(payload, dict) or not isinstance(payload.get("scenes"), list):
        raise ToolFailure("NATIVEVN_SCREENPLAY_INVALID", "screenplay.zh-Hans.json must contain a scenes array")
    lines: dict[str, str] = {}
    for scene in payload["scenes"]:
        if not isinstance(scene, dict) or not isinstance(scene.get("lines"), list):
            continue
        for line in scene["lines"]:
            if not isinstance(line, dict) or not isinstance(line.get("id"), str) or not isinstance(line.get("text"), str):
                continue
            if line["id"] in lines:
                raise ToolFailure("NATIVEVN_SCREENPLAY_LINE_DUPLICATE", "screenplay line ids must be unique")
            lines[line["id"]] = line["text"]
    return lines


def _synthesize(api_key: str, cue: dict[str, Any], model: str, timeout: float) -> bytes:
    voice_id = urllib.parse.quote(cue["voice_id"], safe="")
    endpoint = f"https://api.elevenlabs.io/v1/text-to-speech/{voice_id}"
    request_payload = {
        "text": cue["resolved_text"],
        "model_id": model,
        "voice_settings": cue.get("voice_settings", {"stability": 0.55, "similarity_boost": 0.72, "style": 0.0, "use_speaker_boost": True}),
    }
    request = urllib.request.Request(
        endpoint,
        data=json.dumps(request_payload, ensure_ascii=False).encode("utf-8"),
        headers={"Accept": "audio/mpeg", "Content-Type": "application/json", "xi-api-key": api_key},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            content_type = response.headers.get_content_type()
            audio = response.read()
    except urllib.error.HTTPError as error:
        raise ToolFailure("NATIVEVN_ELEVENLABS_REQUEST_REJECTED", f"ElevenLabs rejected voice cue '{cue['id']}' with HTTP {error.code}") from error
    except (urllib.error.URLError, TimeoutError) as error:
        raise ToolFailure("NATIVEVN_ELEVENLABS_UNAVAILABLE", f"ElevenLabs request failed for voice cue '{cue['id']}'") from error
    if content_type not in {"audio/mpeg", "audio/mp3", "application/octet-stream"} or len(audio) < 256:
        raise ToolFailure("NATIVEVN_ELEVENLABS_RESPONSE_INVALID", f"ElevenLabs returned invalid audio for voice cue '{cue['id']}'")
    return audio


def _probe_audio(path: Path) -> dict[str, int | str]:
    completed = subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "a:0", "-show_entries", "stream=codec_name,sample_rate,channels:format=duration", "-of", "json", str(path)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    try:
        payload = json.loads(completed.stdout)
        stream = payload["streams"][0]
        return {
            "mime_type": "audio/mpeg",
            "codec": stream["codec_name"],
            "sample_rate_hz": int(stream["sample_rate"]),
            "channels": int(stream["channels"]),
            "duration_ms": round(float(payload["format"]["duration"]) * 1000),
        }
    except (KeyError, IndexError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise ToolFailure("NATIVEVN_VOICE_MEDIA_PROBE_FAILED", "ffprobe could not inspect a generated voice candidate", path=path.name) from error


def _validate_voice_metadata(metadata: Any) -> None:
    if not isinstance(metadata, dict):
        raise ToolFailure("NATIVEVN_ELEVENLABS_VOICE_METADATA_INVALID", "ElevenLabs voice metadata is invalid")
    category = metadata.get("category")
    if category == "cloned":
        raise ToolFailure("NATIVEVN_VOICE_CLONING_FORBIDDEN", "cloned ElevenLabs voices are forbidden")
    if category not in {"premade", "generated", "professional"}:
        raise ToolFailure("NATIVEVN_ELEVENLABS_VOICE_CATEGORY_UNPROVEN", "voice category cannot be proven non-cloned")


def _verify_voice_not_cloned(api_key: str, voice_id: str, timeout: float) -> None:
    encoded_voice_id = urllib.parse.quote(voice_id, safe="")
    request = urllib.request.Request(
        f"https://api.elevenlabs.io/v1/voices/{encoded_voice_id}",
        headers={"Accept": "application/json", "xi-api-key": api_key},
        method="GET",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            metadata = json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        raise ToolFailure("NATIVEVN_ELEVENLABS_VOICE_LOOKUP_REJECTED", f"ElevenLabs rejected voice metadata lookup with HTTP {error.code}") from error
    except (urllib.error.URLError, TimeoutError, UnicodeError, json.JSONDecodeError) as error:
        raise ToolFailure("NATIVEVN_ELEVENLABS_VOICE_METADATA_INVALID", "ElevenLabs voice metadata lookup failed") from error
    _validate_voice_metadata(metadata)


def _ensure_private_output_is_ignored(pack_root: Path) -> None:
    try:
        repository = subprocess.run(
            ["git", "-C", str(pack_root), "rev-parse", "--is-inside-work-tree"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
    except OSError as error:
        raise ToolFailure("NATIVEVN_GIT_UNAVAILABLE", "Git is required to prove private voice output is ignored") from error
    if repository.returncode != 0:
        return
    ignored = subprocess.run(
        ["git", "-C", str(pack_root), "check-ignore", "--quiet", "--", ".local/voice/.privacy-probe"],
        capture_output=True,
        check=False,
    )
    if ignored.returncode != 0:
        raise ToolFailure(
            "NATIVEVN_PRIVATE_VOICE_NOT_IGNORED",
            ".local/voice must be covered by Git ignore rules before any private voice request is sent",
            path=".local/voice",
        )


def generate_voice(pack_root: Path, *, model: str = DEFAULT_MODEL, timeout: float = 60.0) -> dict[str, Any]:
    api_key = os.environ.get(API_KEY_ENV, "").strip()
    if not api_key:
        raise ToolFailure("NATIVEVN_ELEVENLABS_API_KEY_MISSING", f"{API_KEY_ENV} is required; no placeholder voice was generated")
    cue_path = pack_root / "Narrative" / "voice-cues.json"
    if not cue_path.is_file():
        raise ToolFailure("NATIVEVN_VOICE_CUES_MISSING", "Narrative/voice-cues.json is required")
    cue_payload = load_json(cue_path)
    actors = cue_payload.get("actors", []) if isinstance(cue_payload, dict) else []
    for actor in actors:
        if not isinstance(actor, dict) or actor.get("speaker_id") not in VOICE_ID_ENV:
            continue
        voice_id = os.environ.get(VOICE_ID_ENV[actor["speaker_id"]], "").strip()
        if voice_id:
            actor["elevenlabs_voice_id"] = voice_id
            actor["voice_source"] = "elevenlabs_voice_design"
    cues = _validate_cues(cue_payload, line_texts=_load_line_texts(pack_root))
    _ensure_private_output_is_ignored(pack_root)
    for voice_id in sorted({cue["voice_id"] for cue in cues}):
        _verify_voice_not_cloned(api_key, voice_id, timeout)
    output_root = pack_root / ".local" / "voice"
    output_root.mkdir(parents=True, exist_ok=True)
    assets: list[dict[str, Any]] = []
    public_cues: list[dict[str, Any]] = []
    for cue in cues:
        audio = _synthesize(api_key, cue, model, timeout)
        destination = output_root / f"{cue['id']}.mp3"
        temporary = destination.with_name(f".{destination.name}.tmp")
        temporary.write_bytes(audio)
        os.replace(temporary, destination)
        assets.append(
            {
                "id": cue["id"],
                "speaker": cue.get("speaker_id") or cue.get("speaker", "unspecified"),
                "locale": cue.get("locale", "zh"),
                "path": destination.name,
                "format": "mp3",
                "sha256": sha256_file(destination),
                "byte_size": destination.stat().st_size,
                "release_eligible": False,
                "license_status": "noncommercial_private",
            }
        )
        settings = cue.get("voice_settings", {"stability": 0.55, "similarity_boost": 0.72, "style": 0.0, "use_speaker_boost": True})
        public_cues.append(
            {
                "id": cue["id"],
                "role": "dialogue",
                "scene_id": cue["scene_id"],
                "route_id": f"route_{cue.get('route_scope', 'common')}",
                "line_id": cue["line_id"],
                "speaker_id": cue["speaker_id"],
                "take": 1,
                "asset": {"private_alias": f"private://voice/{cue['id']}.mp3"},
                "sha256": sha256_file(destination),
                "byte_size": destination.stat().st_size,
                "media": _probe_audio(destination),
                "generation_source": {
                    "kind": "ai_assisted",
                    "method": "voice-design",
                    "tool": "elevenlabs",
                    "model_id": model,
                    "voice_id": cue["voice_id"],
                    "request_parameters": settings,
                },
                "license_status": "blocked_voice_rights",
                "license_evidence": {"relative_path": "README.md"},
                "release_eligible": False,
            }
        )
        print(json.dumps({"event": "voice.generated", "id": cue["id"]}, sort_keys=True))
    manifest = {
        "schema": "astra.nativevn_flagship.voice_manifest.v1",
        "id": "nativevn-flagship-private-voice",
        "model": model,
        "asset_count": len(assets),
        "release_eligible": False,
        "license_status": "noncommercial_private",
        "assets": assets,
    }
    write_json_atomic(output_root / VOICE_MANIFEST, manifest)
    write_json_atomic(
        pack_root / "Manifests" / PUBLIC_METADATA,
        {
            "schema": "nativevn.flagship_voice_cues.v1",
            "package_id": "nativevn.flagship_content",
            "status": "blocked_voice_rights",
            "cues": public_cues,
        },
    )
    return manifest


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pack-root", type=Path, default=_default_pack_root())
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--timeout", type=float, default=60.0)
    args = parser.parse_args(argv)
    diagnostics = Diagnostics()
    try:
        manifest = generate_voice(args.pack_root.resolve(), model=args.model, timeout=args.timeout)
    except ToolFailure as error:
        diagnostics.error(error.code, error.message, error.path)
        diagnostics.emit_json()
        return 2
    except OSError:
        diagnostics.error("NATIVEVN_VOICE_IO_FAILED", "voice generation failed during a filesystem or process operation")
        diagnostics.emit_json()
        return 2
    diagnostics.emit_json(summary={"voice_count": manifest["asset_count"], "release_eligible": False})
    return 0


if __name__ == "__main__":
    sys.exit(main())
