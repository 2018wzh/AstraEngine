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

from projectorrays_json import loads_projectorrays_json

__all__ = ['IMAGE_EXTS', 'AUDIO_EXTS', 'VOICE_HINTS', 'MOVIE_EXTS', 'FONT_EXTS', 'TEXT_EXTS', 'BACKGROUND_HINTS', 'CHARACTER_HINTS', 'TEXT_WINDOW_HINTS', 'BUTTON_HINTS', 'UI_HINTS', 'DIRECTOR_CONTAINER_EXTS', 'READABLE_EXTRACT_EXTS', 'READABLE_RIFF_SIGNATURES', 'SCRIPT_TEXT_CHUNK_IDS', 'DIRECTOR_LINGO_CHUNK_IDS', 'METADATA_JSON_SCHEMAS', 'DEFAULT_STAGE3_TARGETS', 'INTERNAL_DEMO_STAGE3_TARGETS', 'DEMO_SLICE_CONFIG_TEMPLATE', 'DIRECTOR_CAST_MEMBER_METADATA_SCHEMA', 'CAST_MEMBER_KINDS', 'MOUNT_ASSET_ROLES', 'NATIVE_ASSET_BUCKETS', 'SCRIPT_ROUTE_RE', 'PROJECTORRAYS_SCRIPT_SOURCE_RE', 'PROJECTORRAYS_GO_ROUTE_SOURCE_RE', 'PROJECTORRAYS_REQUIRED_CHUNK_ROLES', 'PROJECTORRAYS_JSON_METADATA_CHUNKS', 'SCRIPT_SOURCE_MAP_FORBIDDEN_KEYS', 'TSUINOSORA_REFERENCE_HASHES', 'TSUINOSORA_REFERENCE_DIMENSIONS', '_sha256', '_sha256_bytes', '_fourcc', '_slice_embedded_payload', '_slice_metadata_json_payload', '_slice_script_text_payload', '_slice_embedded_script_text_payload', '_embedded_script_line_start', '_decode_script_text', '_looks_like_script_text', '_script_text_starts_cleanly', '_png_payload_end', '_read_text_lossless', '_script_route_marker', '_parse_choice_list', '_safe_identifier', 'loads_projectorrays_json']


IMAGE_EXTS = {".png"}
AUDIO_EXTS = {".wav", ".ogg", ".flac", ".mp3"}
VOICE_HINTS = {"voice", "voices", "seiyuu"}
MOVIE_EXTS = {".mp4", ".webm", ".avi", ".mpg", ".mpeg"}
FONT_EXTS = {".ttf", ".otf", ".ttc"}
TEXT_EXTS = {".astra", ".txt", ".ini", ".json", ".csv", ".xml", ".html", ".js", ".cfg", ".scr", ".ls"}
BACKGROUND_HINTS = {"bg", "back", "background", "haikei"}
CHARACTER_HINTS = {"char", "character", "sprite", "tachie", "stand", "face", "pose", "chara"}
TEXT_WINDOW_HINTS = {"text_window", "textbox", "message", "msgwindow", "nameplate", "name_plate"}
BUTTON_HINTS = {"button", "btn", "menuitem", "selected"}
UI_HINTS = {"ui", "window", "frame", "border", "menu", "title"}
DIRECTOR_CONTAINER_EXTS = {".dxr", ".cxt", ".dir", ".dcr", ".cst", ".cct"}
READABLE_EXTRACT_EXTS = IMAGE_EXTS | AUDIO_EXTS | MOVIE_EXTS | FONT_EXTS | TEXT_EXTS
READABLE_RIFF_SIGNATURES = {b"RIFF", b"RIFX"}
SCRIPT_TEXT_CHUNK_IDS = {"Lscr", "scrp", "TEXT", "STXT", "LctX", "STR "}
DIRECTOR_LINGO_CHUNK_IDS = {"Lctx", "Lnam", "Lscr"}
METADATA_JSON_SCHEMAS = {
    "tsuinosora.cast_map.v1",
    "tsuinosora.route_graph.v1",
    "tsuinosora.script_source_map.v1",
    "tsuinosora.projectorrays_dump_manifest.v1",
}
DEFAULT_STAGE3_TARGETS = [
    {
        "target": "tsuinosora-internal-game",
        "profiles": ["classic", "modern"],
        "platforms": ["headless", "windows", "web"],
    },
    {
        "target": "tsuinosora-patch-game",
        "profiles": ["classic", "modern"],
        "platforms": ["headless", "windows", "web"],
    },
]
INTERNAL_DEMO_STAGE3_TARGETS = [
    {
        "target": "tsuinosora-internal-game",
        "profiles": ["classic", "modern"],
        "platforms": ["headless", "windows", "web"],
    },
]
DEMO_SLICE_CONFIG_TEMPLATE = {
    "schema": "tsuinosora.demo_slice_config.v1",
    "original_install_root": "Examples/TsuiNoSora/.local/original",
    "local_work_root": "Examples/TsuiNoSora/.local/work",
    "title_png": "Examples/TsuiNoSora/Docs/Title.png",
    "game_png": "Examples/TsuiNoSora/Docs/Game.png",
    "projectorrays_tool": "Examples/TsuiNoSora/.local/tools/ProjectorRays",
    "projectorrays_dump_root": "Examples/TsuiNoSora/.local/projectorrays-dump",
    "projectorrays_full_dump_roots": [
        {"alias": "root", "path": "Examples/TsuiNoSora/.local/projectorrays-full-root"},
        {"alias": "data", "path": "Examples/TsuiNoSora/.local/projectorrays-full-data"},
        {"alias": "casts", "path": "Examples/TsuiNoSora/.local/projectorrays-full-casts"},
    ],
    "projectorrays_palette_sidecars": ["Examples/TsuiNoSora/.local/palettes/system-win-d5.palette.json"],
    "player_automation_report": "Examples/TsuiNoSora/.local/work/reports/live_player_report.json",
    "player_automation": {
        "schema": "astra.player_live_automation_config.v1",
        "backend": "windows_sendinput",
        "timeout_ms": 60000,
    },
    "require_full_resource_conversion": True,
    "require_visual_screenshot_acceptance": True,
    "visual_capture": {
        "schema": "tsuinosora.visual_capture_config.v1",
        "thresholds": {"max_mean_delta": 4.0, "max_changed_ratio": 0.05},
        "capture_automation": {
            "schema": "tsuinosora.visual_capture_automation.v1",
            "backend": "windows_sendinput",
            "sessions": [
                {
                    "role": "original",
                    "launch": {
                        "command": ["Examples/TsuiNoSora/.local/original/TsuiNoSora.exe"],
                        "working_directory": "Examples/TsuiNoSora/.local/original",
                    },
                    "window_match": {"title_contains": "TsuiNoSora", "process_name": "TsuiNoSora.exe"},
                    "startup_timeout_ms": 15000,
                },
                {
                    "role": "demo",
                    "launch": {
                        "command": ["AstraPlayer.exe"],
                        "working_directory": "Examples/TsuiNoSora/.local/work/bundles/internal-classic/windows",
                    },
                    "window_match": {"title_contains": "AstraPlayer", "process_name": "AstraPlayer.exe"},
                    "startup_timeout_ms": 60000,
                },
            ],
            "input_scripts": [
                {
                    "checkpoint_id": "title",
                    "steps": [
                        {"kind": "wait", "duration_ms": 1000},
                        {"kind": "capture", "role": "original"},
                        {"kind": "capture", "role": "demo"},
                    ],
                },
                {
                    "checkpoint_id": "first_dialogue",
                    "steps": [
                        {"kind": "key", "key": "enter"},
                        {"kind": "wait", "duration_ms": 1000},
                        {"kind": "capture", "role": "original"},
                        {"kind": "capture", "role": "demo"},
                    ],
                },
            ],
        },
        "checkpoints": [
            {
                "checkpoint_id": "title",
                "route_id": "classic.title",
                "required": True,
                "original_screenshot": "screenshots/original/title.png",
                "demo_screenshot": "screenshots/demo/title.png",
                "regions": [
                    {"region_id": "full_frame", "x": 0, "y": 0, "width": 0, "height": 0, "required": True}
                ],
            },
            {
                "checkpoint_id": "first_dialogue",
                "route_id": "classic.main",
                "required": True,
                "original_screenshot": "screenshots/original/first_dialogue.png",
                "demo_screenshot": "screenshots/demo/first_dialogue.png",
                "regions": [
                    {"region_id": "background_viewport", "x": 0, "y": 0, "width": 0, "height": 0, "required": True},
                    {"region_id": "text_window", "x": 0, "y": 0, "width": 0, "height": 0, "required": True},
                ],
            },
        ],
        "visual_reviews": [],
    },
}
DIRECTOR_CAST_MEMBER_METADATA_SCHEMA = "tsuinosora.director_cast_member_metadata.v1"
CAST_MEMBER_KINDS = {
    "background",
    "character_sprite",
    "character_atlas",
    "cg",
    "ui",
    "text_window",
    "button",
    "audio",
    "voice",
    "movie",
    "font",
    "script",
    "unknown",
}
MOUNT_ASSET_ROLES = CAST_MEMBER_KINDS - {"script", "unknown"}
NATIVE_ASSET_BUCKETS = {
    "background": "backgrounds",
    "character_sprite": "characters/sprites",
    "character_atlas": "characters/atlases",
    "cg": "cg",
    "ui": "ui",
    "text_window": "ui/text_windows",
    "button": "ui/buttons",
    "audio": "audio",
    "voice": "voice",
    "movie": "movies",
    "font": "fonts",
}
SCRIPT_ROUTE_RE = re.compile(
    r"^\s*(?:#|--|//)?\s*(?:astra[\s._-]*)?route\s*[:\s]\s*"
    r"(?P<route>[A-Za-z0-9_.-]+)"
    r"(?:\s*(?:->|terminal\s*[:=]?)\s*(?P<terminal>[A-Za-z0-9_.-]+))?"
    r"(?:.*?\bchoices?\s*[:=]?\s*(?P<choices>[A-Za-z0-9_., -]+))?",
    re.IGNORECASE,
)
PROJECTORRAYS_SCRIPT_SOURCE_RE = re.compile(
    r"^(BehaviorScript|MovieScript|CastScript|ParentScript)\s+(\d+)(?:\s+-.*)?$",
    re.IGNORECASE,
)
PROJECTORRAYS_GO_ROUTE_SOURCE_RE = re.compile(
    r"^(BehaviorScript|MovieScript|CastScript|ParentScript)\s+\d+\s+-\s*GO\[([A-Za-z0-9_]+)\]$",
    re.IGNORECASE,
)
PROJECTORRAYS_REQUIRED_CHUNK_ROLES = {
    "BITD": "bitmap_or_palette_backed_image",
    "CASt": "cast_member_metadata",
    "STXT": "text_or_field_member",
    "Lscr": "lingo_script_bytecode",
    "SCRF": "script_context_reference",
    "snd ": "sound_media",
    "sndH": "sound_header",
    "sndS": "sound_sample_data",
    "ediM": "embedded_media",
    "XMED": "xtra_media_metadata",
    "CAS_": "cast_member_binding",
    "KEY_": "resource_key_table",
    "Lctx": "lingo_context_table",
    "Lnam": "lingo_name_table",
    "mmap": "resource_map",
    "imap": "initial_map",
    "Cinf": "cast_info_table",
    "DRCF": "director_config",
    "Fmap": "font_map",
    "FCOL": "color_palette",
    "FXmp": "font_xtra_map",
    "MCsL": "movie_cast_list",
    "Sord": "score_order",
    "VERS": "director_version",
    "VWFI": "view_frame_info",
    "VWLB": "view_label_table",
    "VWSC": "view_score",
    "XTRl": "xtra_list",
    "cupt": "cue_point_table",
}
PROJECTORRAYS_JSON_METADATA_CHUNKS = {
    "CAS_",
    "CASt",
    "Cinf",
    "DRCF",
    "FCOL",
    "FXmp",
    "Fmap",
    "KEY_",
    "Lctx",
    "Lnam",
    "MCsL",
    "SCRF",
    "Sord",
    "VERS",
    "VWFI",
    "VWLB",
    "VWSC",
    "XTRl",
    "cupt",
    "imap",
    "mmap",
}
SCRIPT_SOURCE_MAP_FORBIDDEN_KEYS = {
    "body",
    "bytecode",
    "bytes",
    "commercial_text",
    "content",
    "lingo_source",
    "payload",
    "payload_bytes",
    "script_text",
    "source_text",
    "text",
}
TSUINOSORA_REFERENCE_HASHES = {
    "title": "sha256:3799183a831bdbdc144e1bc9e06dffd831417d436338a1daf04b45bc35624bca",
    "game": "sha256:1c4ddf68fa15fd6a76db259b155366456198bd551c49de8a9ede9ca0f2be9d84",
}
TSUINOSORA_REFERENCE_DIMENSIONS = {
    "title": {"width": 1386, "height": 1040},
    "game": {"width": 1403, "height": 1053},
}

def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def _sha256_bytes(value: bytes) -> str:
    return "sha256:" + hashlib.sha256(value).hexdigest()


def _fourcc(value: bytes) -> str:
    if len(value) != 4:
        return value.hex()
    try:
        decoded = value.decode("ascii")
    except UnicodeDecodeError:
        return value.hex()
    if all(32 <= ord(ch) <= 126 for ch in decoded):
        return decoded
    return value.hex()


def _slice_embedded_payload(payload: bytes) -> tuple[bytes, str, str, int] | None:
    png_offset = payload.find(b"\x89PNG\r\n\x1a\n")
    if png_offset >= 0:
        end = _png_payload_end(payload, png_offset)
        if end:
            return payload[png_offset:end], "png", "image_png", png_offset

    riff_offset = payload.find(b"RIFF")
    if riff_offset >= 0 and len(payload) >= riff_offset + 12 and payload[riff_offset + 8 : riff_offset + 12] == b"WAVE":
        size = struct.unpack("<I", payload[riff_offset + 4 : riff_offset + 8])[0] + 8
        end = min(len(payload), riff_offset + size)
        return payload[riff_offset:end], "wav", "audio", riff_offset

    signatures = [
        (b"OggS", "ogg", "audio"),
        (b"fLaC", "flac", "audio"),
        (b"ID3", "mp3", "audio"),
        (b"\xff\xfb", "mp3", "audio"),
        (b"\x00\x00\x00\x18ftyp", "mp4", "movie"),
    ]
    for signature, extension, probe in signatures:
        offset = payload.find(signature)
        if offset >= 0:
            return payload[offset:], extension, probe, offset
    return None


def _slice_metadata_json_payload(payload: bytes) -> tuple[str, str, int] | None:
    stripped = payload.strip(b"\x00\r\n\t ")
    if not stripped or b"{" not in stripped:
        return None
    offset = stripped.find(b"{")
    candidate = stripped[offset:]
    decoded = _decode_script_text(candidate)
    if not decoded:
        return None
    text, _encoding = decoded
    try:
        value = json.loads(text)
    except json.JSONDecodeError:
        return None
    if not isinstance(value, dict) or value.get("schema") not in METADATA_JSON_SCHEMAS:
        return None
    normalized = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    original_offset = payload.find(stripped) + offset
    return normalized, value["schema"], max(original_offset, 0)


def _slice_script_text_payload(payload: bytes, chunk_id: str) -> tuple[str, str, int] | None:
    if chunk_id not in SCRIPT_TEXT_CHUNK_IDS:
        return None
    stripped = payload.strip(b"\x00\r\n\t ")
    if len(stripped) < 4:
        return None
    decoded = _decode_script_text(stripped)
    if decoded:
        text, encoding = decoded
        if _looks_like_script_text(text) and _script_text_starts_cleanly(text):
            offset = payload.find(stripped)
            return text, encoding, max(offset, 0)
    return _slice_embedded_script_text_payload(payload)


def _slice_embedded_script_text_payload(payload: bytes) -> tuple[str, str, int] | None:
    lower = payload.lower()
    offsets = []
    for marker in (b"astra route", b"astra_route", b"astra-route", b"route:"):
        search_from = 0
        while True:
            marker_offset = lower.find(marker, search_from)
            if marker_offset < 0:
                break
            start = _embedded_script_line_start(payload, marker_offset)
            if start not in offsets:
                offsets.append(start)
            search_from = marker_offset + len(marker)
    for start in sorted(offsets):
        candidate = payload[start:].strip(b"\x00\r\n\t ")
        if len(candidate) < 4:
            continue
        decoded = _decode_script_text(candidate)
        if not decoded:
            continue
        text, encoding = decoded
        if _looks_like_script_text(text):
            inner_offset = start + max(payload[start:].find(candidate), 0)
            return text, encoding, inner_offset
    return None


def _embedded_script_line_start(payload: bytes, marker_offset: int) -> int:
    line_start = payload.rfind(b"\n", 0, marker_offset) + 1
    prefix = payload[line_start:marker_offset]
    starts = [prefix.rfind(token) for token in (b"--", b"//", b"#")]
    best = max(starts)
    if best >= 0:
        return line_start + best
    return marker_offset


def _decode_script_text(payload: bytes) -> tuple[str, str] | None:
    for encoding in ("utf-8-sig", "cp932", "shift_jis"):
        try:
            text = payload.decode(encoding)
        except UnicodeDecodeError:
            continue
        return text.replace("\r\n", "\n").replace("\r", "\n"), encoding
    return None


def _looks_like_script_text(text: str) -> bool:
    if not text.strip():
        return False
    printable = 0
    controls = 0
    for ch in text:
        if ch in "\n\t" or ch.isprintable():
            printable += 1
        else:
            controls += 1
    return printable > 0 and controls / max(printable + controls, 1) < 0.05


def _script_text_starts_cleanly(text: str) -> bool:
    stripped = text.lstrip("\ufeff\r\n\t ")
    return bool(stripped) and (stripped[0].isprintable() or stripped[0] in "\n\t")


def _png_payload_end(payload: bytes, start: int) -> int | None:
    offset = start + 8
    while offset + 12 <= len(payload):
        length = struct.unpack(">I", payload[offset : offset + 4])[0]
        kind = payload[offset + 4 : offset + 8]
        next_offset = offset + 12 + length
        if next_offset > len(payload):
            return None
        offset = next_offset
        if kind == b"IEND":
            return offset
    return None


def _read_text_lossless(path: Path) -> str:
    data = path.read_bytes()
    decoded = _decode_script_text(data)
    if decoded:
        return decoded[0]
    return data.decode("utf-8", errors="ignore").replace("\r\n", "\n").replace("\r", "\n")


def _script_route_marker(line: str) -> dict | None:
    match = SCRIPT_ROUTE_RE.match(line)
    if not match:
        return None
    route_id = match.group("route")
    terminal = match.group("terminal") or f"ending.{_safe_identifier(route_id)}"
    choices = _parse_choice_list(match.group("choices") or "")
    return {
        "route_id": route_id,
        "coverage": "covered",
        "terminal": terminal,
        "choices": choices,
    }


def _parse_choice_list(value: str) -> list[str]:
    if not value:
        return []
    return [
        item.strip()
        for item in re.split(r"[, ]+", value)
        if item.strip() and re.match(r"^[A-Za-z0-9_.-]+$", item.strip())
    ]


def _safe_identifier(value: str) -> str:
    cleaned = "".join(ch if ch.isalnum() else "_" for ch in value.lower()).strip("_")
    return cleaned or "route"


