#!/usr/bin/env python3
"""Refresh NativeVN derived assets and descriptors; .astra sources stay authoritative."""

from __future__ import annotations

import hashlib
import json
import shutil
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PACK = ROOT / "Examples" / "NativeVN"
PROFILE = "advanced-vn"

SYSTEM_STRINGS = {
    "speaker.lin_yao": {"zh-Hans": "林瑶", "en": "Lin Yao"},
    "speaker.zhou_heng": {"zh-Hans": "周衡", "en": "Zhou Heng"},
    "system.back": {"zh-Hans": "返回", "en": "Back"},
    "system.continue": {"zh-Hans": "继续", "en": "Continue"},
    "system.title": {"zh-Hans": "玻璃雨中的信号", "en": "Signal in the Glass Rain"},
    "system.save": {"zh-Hans": "保存", "en": "Save"},
    "system.load": {"zh-Hans": "读取", "en": "Load"},
    "system.config": {"zh-Hans": "设置", "en": "Settings"},
    "system.config.master_volume": {"zh-Hans": "主音量", "en": "Master Volume"},
    "system.config.text_speed": {"zh-Hans": "文字速度", "en": "Text Speed"},
    "system.config.auto_delay": {"zh-Hans": "自动播放延迟", "en": "Auto Delay"},
    "system.config.high_contrast": {"zh-Hans": "高对比度", "en": "High Contrast"},
    "system.gallery": {"zh-Hans": "鉴赏", "en": "Gallery"},
    "system.replay": {"zh-Hans": "回放", "en": "Replay"},
    "system.voice_replay": {"zh-Hans": "语音回放", "en": "Voice Replay"},
    "system.route_chart": {"zh-Hans": "路线图", "en": "Route Chart"},
    "system.backlog": {"zh-Hans": "历史记录", "en": "Backlog"},
    "system.localization_preview": {"zh-Hans": "本地化预览", "en": "Localization Preview"},
}


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def write_json(path: Path, payload: dict) -> None:
    write_text(path, json.dumps(payload, ensure_ascii=False, indent=2) + "\n")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def asset_id(source: str) -> str:
    path = Path(source)
    parts = list(path.with_suffix("").parts)
    if parts[:2] == ["Visual", "Backgrounds"]:
        return f"asset:/background/{parts[-1]}"
    if parts[:2] == ["Visual", "CG"]:
        return f"asset:/cg/{parts[-1]}"
    if parts[:2] == ["Visual", "Characters"]:
        normalized = "/".join(part.lower() for part in parts[2:])
        return f"asset:/character/{normalized}"
    if parts[:2] == ["Visual", "Endings"]:
        return f"asset:/ending/{parts[-1]}"
    if parts[:2] == ["Visual", "Gallery"]:
        return f"asset:/gallery/{'/'.join(part.lower() for part in parts[2:])}"
    if parts[:2] == ["Visual", "KeyArt"]:
        return f"asset:/key-art/{parts[-1]}"
    if parts[:2] == ["Visual", "UI"]:
        return f"asset:/ui/{parts[-1]}"
    if parts[:2] == ["Visual", "Video"]:
        return f"asset:/movie/{parts[-1]}"
    if parts[:3] == ["Audio", "Distribution", "bgm"]:
        return f"asset:/bgm/{parts[-1]}"
    if parts[:3] == ["Audio", "Distribution", "se"]:
        return f"asset:/se/{parts[-1]}"
    if parts[:3] == ["Audio", "Distribution", "stinger"]:
        return f"asset:/stinger/{parts[-1]}"
    if parts[:3] == ["Audio", "Voice", "Distribution"]:
        return f"asset:/voice/{parts[-1]}"
    if parts[:2] == ["Assets", "Fonts"]:
        return f"asset:/font/{parts[-1].lower()}"
    raise ValueError(f"unsupported project asset source: {source}")


def asset_type(source: str) -> tuple[str, str, str]:
    suffix = Path(source).suffix.lower()
    if suffix == ".png":
        return "image.png", "astra.import.image", "astra.cook.texture2d"
    if suffix == ".ogg":
        return "audio.ogg", "astra.import.audio", "astra.cook.audio"
    if suffix == ".webm":
        return "video.webm", "astra.import.video", "astra.cook.video"
    if suffix == ".ttf":
        return "font.ttf", "astra.import.font", "astra.cook.font"
    raise ValueError(f"unsupported asset type: {source}")


def discover_runtime_assets() -> list[str]:
    sources = []
    for path in sorted((PACK / "Visual").rglob("*.png")):
        relative = path.relative_to(PACK).as_posix()
        if "/Video/Source/" not in f"/{relative}":
            sources.append(relative)
    sources.append("Visual/Video/rain-signal-loop.webm")
    for category in ("bgm", "se", "stinger"):
        for path in sorted((PACK / "Audio" / "Distribution" / category).glob("*.ogg")):
            sources.append(path.relative_to(PACK).as_posix())
    for path in sorted((PACK / "Audio" / "Voice" / "Distribution").rglob("*.ogg")):
        sources.append(path.relative_to(PACK).as_posix())
    sources.append("Assets/Fonts/NotoSansJP-Variable.ttf")
    return sources


def build_sidecars() -> None:
    sidecar_root = PACK / "AssetSidecars"
    if sidecar_root.exists():
        shutil.rmtree(sidecar_root)
    for source in discover_runtime_assets():
        source_path = PACK / source
        kind, importer, processor = asset_type(source)
        lines = [
            "schema: astra.asset.v1",
            f"id: {asset_id(source)}",
            f"source: {source}",
            f"source_hash: sha256:{sha256(source_path)}",
            f"type: {kind}",
            "license: OFL-1.1" if kind.startswith("font.") else "license: project-owned-generated",
            f"importer: {importer}",
        ]
        if kind.startswith("font."):
            lines.extend([
                "font:",
                "  family: Noto Sans JP",
                "  face_index: 0",
                "  subset: cjk-production",
                "  coverage:",
                "    - { start: 32, end: 126 }",
                "    - { start: 12288, end: 12543 }",
                "    - { start: 13312, end: 40959 }",
                "    - { start: 65280, end: 65519 }",
            ])
        lines.extend([
            "cook:",
            f"  processor: {processor}",
            f"  target_profiles: [{PROFILE}]",
            "  params: { color_space: srgb }" if suffix_is_image(source) else "  params: {}",
            "review: accepted",
            "",
        ])
        name = asset_id(source).removeprefix("asset:/").replace("/", "__") + ".astra-asset.yaml"
        write_text(sidecar_root / name, "\n".join(lines))


def suffix_is_image(source: str) -> bool:
    return Path(source).suffix.lower() == ".png"


def build_localization() -> None:
    payloads = {
        "zh-Hans": read_json(PACK / "Narrative" / "screenplay.zh-Hans.json"),
        "en": read_json(PACK / "Localization" / "screenplay.en.json"),
    }
    for locale, screenplay in payloads.items():
        strings = {key: value[locale] for key, value in SYSTEM_STRINGS.items()}
        for scene in screenplay["scenes"]:
            for line in scene["lines"]:
                strings[f"line.{line['id']}"] = line["text"]
        for choice in screenplay["choices"]:
            strings[f"choice.{choice['id']}"] = choice["prompt"]
            for option in choice["options"]:
                strings[f"choice.{option['id']}"] = option["text"]
        write_json(PACK / "Localization" / f"{locale}.json", {
            "schema": "astra.vn.localization_table.v1",
            "locale": locale,
            "strings": dict(sorted(strings.items())),
        })


def build_ui() -> None:
    required = [
        PACK / "UI" / "flagship.astra",
        PACK / "Controllers" / "standard_ui.luau",
        PACK / "Themes" / "classic.json",
        PACK / "Themes" / "modern.json",
        PACK / "Scripts" / "system.astra",
    ]
    missing = [path.relative_to(PACK).as_posix() for path in required if not path.is_file()]
    if missing:
        raise FileNotFoundError(f"self-contained UI sources are missing: {', '.join(missing)}")


def build_project_descriptor() -> None:
    write_text(PACK / "project.yaml", """schema: astra.target_manifest.v2
id: com.astra.nativevn.signal-glass-rain
platform_profiles:
  windows-release:
    schema: astra.platform_host_profile.v3
    id: windows-release
    platform: windows
    target: nativevn-flagship-game
    package_id: com.astra.nativevn.signal-glass-rain
    renderer: { providers: [wgpu_hardware], allow_software: false }
    decode: { providers: [wmf], allow_software: false }
    audio_mixer: { providers: [kira], allow_software: false }
    audio_output: { providers: [wasapi], allow_software: false }
    save: { providers: [saved_games], allow_software: false }
    package_sources: [{ kind: bundled }, { kind: user_authorized }]
    limits: { command_queue_capacity: 256, event_queue_capacity: 1024, max_frame_bytes: 67108864, max_audio_frames: 192000, audio_pcm_cache_bytes: 268435456, audio_chunk_frames: 512, max_package_read_bytes: 8388608 }
    package_cache: { max_entry_bytes: 17179869184, max_total_bytes: 68719476736 }
  linux-steam-sniper-release:
    schema: astra.platform_host_profile.v3
    id: linux-steam-sniper-release
    platform: linux
    target: nativevn-flagship-game
    package_id: com.astra.nativevn.signal-glass-rain
    renderer: { providers: [wgpu_vulkan], allow_software: false }
    decode: { providers: [gstreamer], allow_software: false }
    audio_mixer: { providers: [kira], allow_software: false }
    audio_output: { providers: [alsa], allow_software: false }
    save: { providers: [xdg_data], allow_software: false }
    package_sources: [{ kind: bundled }, { kind: user_authorized }]
    limits: { command_queue_capacity: 256, event_queue_capacity: 1024, max_frame_bytes: 67108864, max_audio_frames: 192000, audio_pcm_cache_bytes: 268435456, audio_chunk_frames: 512, max_package_read_bytes: 8388608 }
    package_cache: { max_entry_bytes: 17179869184, max_total_bytes: 68719476736 }
  macos-release:
    schema: astra.platform_host_profile.v3
    id: macos-release
    platform: macos
    target: nativevn-flagship-game
    package_id: com.astra.nativevn.signal-glass-rain
    renderer: { providers: [wgpu_metal], allow_software: false }
    decode: { providers: [avfoundation], allow_software: false }
    audio_mixer: { providers: [kira], allow_software: false }
    audio_output: { providers: [coreaudio], allow_software: false }
    save: { providers: [application_support], allow_software: false }
    package_sources: [{ kind: bundled }, { kind: user_authorized }]
    limits: { command_queue_capacity: 256, event_queue_capacity: 1024, max_frame_bytes: 67108864, max_audio_frames: 192000, audio_pcm_cache_bytes: 268435456, audio_chunk_frames: 512, max_package_read_bytes: 8388608 }
    package_cache: { max_entry_bytes: 17179869184, max_total_bytes: 68719476736 }
  android-release:
    schema: astra.platform_host_profile.v3
    id: android-release
    platform: android
    target: nativevn-flagship-game
    package_id: com.astra.nativevn.signal-glass-rain
    renderer: { providers: [wgpu_vulkan], allow_software: false }
    decode: { providers: [mediacodec], allow_software: false }
    audio_mixer: { providers: [kira], allow_software: false }
    audio_output: { providers: [oboe_aaudio, oboe_opensl_es], allow_software: false }
    save: { providers: [android_app_storage], allow_software: false }
    package_sources:
      - { kind: bundled }
      - { kind: user_authorized }
    limits: { command_queue_capacity: 256, event_queue_capacity: 1024, max_frame_bytes: 67108864, max_audio_frames: 192000, audio_pcm_cache_bytes: 268435456, audio_chunk_frames: 512, max_package_read_bytes: 8388608 }
    package_cache: { max_entry_bytes: 17179869184, max_total_bytes: 68719476736 }
targets:
  - id: nativevn-flagship-game
    kind: game
    crate: astra-vn
    default_profile: advanced-vn
    runtime_provider: native_vn
    ui_provider: astra.ui.yakui
    platforms: [windows, linux, macos, android]
    packaged: true
nativevn:
  default_locale: zh-Hans
  sources: [Scripts]
  ui_sources: [UI]
  ui_themes: [Themes]
  ui_controllers: [Controllers]
  profiles: [classic, advanced-vn]
  asset_roots: [AssetSidecars]
  display:
    original_resolution: { width: 1920, height: 1080 }
    scale_filter: linear
    preview_layers:
      - { vfs_uri: "package:/Visual/KeyArt/title-background.png", x: 0, y: 0 }
package_sections:
  - id: vn.localization.zh-Hans
    schema: astra.vn.localization_table.v1
    path: Localization/zh-Hans.json
    codec: raw
    targets: [nativevn-flagship-game]
    profiles: [classic, advanced-vn]
  - id: vn.localization.en
    schema: astra.vn.localization_table.v1
    path: Localization/en.json
    codec: raw
    targets: [nativevn-flagship-game]
    profiles: [classic, advanced-vn]
  - id: vn.flagship.route_graph
    schema: astra.nativevn.flagship.route_graph.v1
    path: Narrative/route-graph.json
    codec: raw
    targets: [nativevn-flagship-game]
    profiles: [classic, advanced-vn]
""")


def copy_fonts() -> None:
    required = [
        PACK / "Assets" / "Fonts" / "NotoSansJP-Variable.ttf",
        PACK / "Assets" / "Fonts" / "OFL-NotoSansJP.txt",
    ]
    missing = [path.relative_to(PACK).as_posix() for path in required if not path.is_file()]
    if missing:
        raise FileNotFoundError(f"self-contained font assets are missing: {', '.join(missing)}")


def main() -> int:
    copy_fonts()
    build_localization()
    build_ui()
    build_sidecars()
    build_project_descriptor()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
