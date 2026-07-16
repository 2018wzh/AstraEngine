#!/usr/bin/env python3
"""Rebuild flagship content manifest, provenance, review, prompts, and alt text."""

from __future__ import annotations

import argparse
import hashlib
import json
import mimetypes
import re
import subprocess
import sys
import wave
from pathlib import Path
from typing import Any

try:
    from PIL import Image
except ImportError as error:  # pragma: no cover - environment preflight
    raise SystemExit("NATIVEVN_METADATA_PIL_MISSING: install Pillow before rebuilding metadata") from error


SCHEMA_REFS = [
    "Schemas/nativevn.flagship_content_manifest.v1.schema.json",
    "Schemas/nativevn.flagship_voice_cues.v1.schema.json",
    "Schemas/nativevn.flagship_provenance.v1.schema.json",
    "Schemas/nativevn.flagship_review.v1.schema.json",
]

BACKGROUND_ALT = {
    "apartment-studio": ("雨夜高层公寓工作室，信号分析仪与两把空椅子面对落雨的玻璃城市。", "A rain-night apartment studio where a signal analyzer and two empty chairs face the glass city."),
    "rain-street": ("暴雨中的玻璃都市街道，青色灯光与一处洋红信号倒影落在湿地面上。", "A glass-city street in heavy rain, with cyan light and one magenta signal reflection on wet pavement."),
    "metro-station": ("空旷的雨夜地铁站，站台玻璃上浮现洋红色异常波形。", "An empty rain-night metro platform with an anomalous magenta waveform across the glass doors."),
    "glass-corridor": ("高空玻璃连廊横跨雨夜城市，一道洋红信号沿窗面穿过。", "An elevated glass corridor above the rainy city, crossed by a thin magenta signal."),
    "archive-room": ("深蓝档案室里排列着玻璃数据柜，一只抽屉发出洋红色微光。", "A deep-blue archive room of glass data drawers, one of them glowing magenta."),
    "rooftop-antenna": ("暴雨天台布满天线阵列，洋红脉冲在城市上空连接设备。", "A storm-lashed rooftop antenna field carrying a magenta pulse above the city."),
    "underground-relay": ("地下中继室的环形核心隔着安全玻璃发光，线缆倒映在积水中。", "An underground relay core glowing behind safety glass, with cables reflected in shallow water."),
    "dawn-city": ("雨后黎明照亮玻璃都市，远方留下一条逐渐淡去的洋红信号。", "Dawn after rain lights the glass metropolis as a magenta signal fades into the horizon."),
}

CG_ALT = {
    "opening-signal": ("林瑶与周衡隔着公寓站立，窗外洋红异常信号横贯雨夜城市。", "Lin Yao and Zhou Heng stand apart in the apartment as a magenta signal crosses the rainy city."),
    "reunion": ("林瑶与周衡在空地铁站重逢，二人手中的分析仪映出同一段异常波形。", "Lin Yao and Zhou Heng reunite on an empty platform while their instruments catch the same anomaly."),
    "anomaly-reveal": ("林瑶打开档案抽屉，周衡操作控制台，删除的共同记忆以光路显现。", "Lin Yao opens an archive drawer while Zhou Heng reveals erased shared memories at the console."),
    "ending-truth": ("真相终局：二人在雨中天台开启公共发射，城市灯光逐点回应。", "Truth ending: the pair open the public transmitter in the rain as city lights answer."),
    "ending-silence": ("静默终局：中继核心熄灭，林瑶与周衡隔着玻璃成为陌生人。", "Silence ending: the relay dies and Lin Yao and Zhou Heng face each other as strangers through glass."),
    "ending-signal": ("信号终局：二人在黎明连廊建立受控通道，一次洋红脉冲通过验证。", "Signal ending: at dawn the pair establish a controlled channel and one magenta pulse passes verification."),
}

UI_ALT = {
    "title": ("无字标题界面视觉稿，雨夜城市上叠加信号徽记与菜单框。", "Textless title-screen mockup with a signal emblem and menu frames over the rainy city."),
    "message": ("对白界面视觉稿，下方烟色玻璃消息框保留角色名与控制图标位置。", "Dialogue-screen mockup with a smoked-glass message panel and reserved speaker and control areas."),
    "choice": ("选择界面视觉稿，三张玻璃选项卡中间一项以青色边框选中。", "Choice-screen mockup with three glass options and the middle option highlighted in cyan."),
    "backlog": ("回看界面视觉稿，交错对白条目与语音回放波形排列在玻璃面板内。", "Backlog mockup with alternating dialogue rows and a voice-replay waveform inside a glass panel."),
    "save": ("存档界面视觉稿，六张缩略图卡片以路线色标区分。", "Save-screen mockup with six thumbnail cards distinguished by route-color markers."),
    "load": ("读档界面视觉稿，选中卡片以青色框和校验图形突出。", "Load-screen mockup with the selected card emphasized by a cyan frame and verification motif."),
    "config": ("设置界面视觉稿，包含分类栏、滑杆、开关与音频分组。", "Configuration mockup with category rail, sliders, toggles, and audio groups."),
    "gallery": ("画廊界面视觉稿，缩略图网格旁显示放大预览与筛选图标。", "Gallery mockup with a thumbnail grid, enlarged preview, and filter icons."),
    "route-chart": ("路线图视觉稿，共通节点分成三条路径并连接三个独立终局。", "Route-chart mockup where the common path splits into three routes and three distinct endings."),
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def safe_id(relative: str) -> str:
    value = re.sub(r"[^a-z0-9.-]+", "-", relative.lower().replace("/", ".")).strip("-.")
    value = re.sub(r"[.-]{2,}", ".", value)
    return f"asset.{value}"[:128].rstrip("-.")


def ffprobe(path: Path) -> dict[str, Any]:
    completed = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "stream=codec_name,width,height,r_frame_rate,sample_rate,channels:format=duration", "-of", "json", str(path)],
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def media_info(path: Path) -> dict[str, Any]:
    suffix = path.suffix.lower()
    mime = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
    if suffix == ".png":
        with Image.open(path) as image:
            return {"kind": "image", "mime_type": "image/png", "width": image.width, "height": image.height, "color_space": "srgb", "has_alpha": "A" in image.getbands()}
    if suffix == ".svg":
        return {"kind": "vector", "mime_type": "image/svg+xml"}
    if suffix == ".wav":
        with wave.open(str(path), "rb") as stream:
            return {"kind": "audio", "mime_type": "audio/wav", "duration_ms": round(1000 * stream.getnframes() / stream.getframerate()), "sample_rate_hz": stream.getframerate(), "channels": stream.getnchannels(), "codec": "pcm-s24le"}
    if suffix in {".ogg", ".mp4", ".webm"}:
        probe = ffprobe(path)
        stream = probe["streams"][0]
        duration_ms = round(float(probe["format"]["duration"]) * 1000)
        if suffix == ".ogg":
            return {"kind": "audio", "mime_type": "audio/ogg", "duration_ms": duration_ms, "sample_rate_hz": int(stream["sample_rate"]), "channels": int(stream["channels"]), "codec": stream["codec_name"]}
        numerator, denominator = (int(value) for value in stream["r_frame_rate"].split("/"))
        return {"kind": "video", "mime_type": "video/mp4" if suffix == ".mp4" else "video/webm", "width": int(stream["width"]), "height": int(stream["height"]), "duration_ms": duration_ms, "frame_rate": numerator / denominator, "codec": stream["codec_name"]}
    return {"kind": "document" if suffix in {".md", ".txt"} else "data", "mime_type": mime}


def role_for(relative: str) -> str:
    lower = relative.lower()
    if "/audio/master/bgm/" in f"/{lower}" or "/audio/distribution/bgm/" in f"/{lower}":
        return "bgm"
    if "/audio/voice/" in f"/{lower}":
        return "voice"
    if "/audio/" in f"/{lower}" and path_kind(lower) in {"wav", "ogg"}:
        return "se"
    if lower.endswith((".mp4", ".webm")) or "/video/" in f"/{lower}":
        return "video"
    if "/characters/" in f"/{lower}":
        return "character"
    if "/fonts/" in f"/{lower}":
        return "font"
    if "/ui/" in f"/{lower}" or "/icons/" in f"/{lower}":
        return "ui_icon"
    if "/backgrounds/" in f"/{lower}" or "/cg/" in f"/{lower}" or "/keyart/" in f"/{lower}" or "/endings/" in f"/{lower}" or "/gallery/" in f"/{lower}":
        return "background"
    if lower.endswith("audio-manifest.json"):
        return "manifest"
    return "review_evidence"


def path_kind(relative: str) -> str:
    return Path(relative).suffix.lower().lstrip(".")


def generation_for(relative: str) -> dict[str, Any]:
    lower = relative.lower()
    if lower.endswith(".svg"):
        return {"kind": "code_native", "method": "svg", "tool": "hand-authored-svg"}
    if "/audio/" in f"/{lower}" and lower.endswith((".wav", ".ogg")):
        if "/voice/" in f"/{lower}":
            return {"kind": "ai_assisted", "method": "text-to-speech", "tool": "openrouter"}
        return {"kind": "code_native", "method": "deterministic-synthesis", "tool": "nativevn-audio-generator"}
    if "/visual/video/" in f"/{lower}":
        return {"kind": "code_native", "method": "layered-video", "tool": "nativevn-video-generator"}
    if "/visual/gallery/thumbnails/" in f"/{lower}":
        return {"kind": "code_native", "method": "ffmpeg-scale", "tool": "ffmpeg"}
    if lower.endswith(".png"):
        method = "imagegen-chroma-key" if "/sprites/" in f"/{lower}" else "imagegen"
        return {"kind": "ai_assisted", "method": method, "tool": "openai-imagegen"}
    return {"kind": "hand_authored", "method": "editorial", "tool": "codex"}


def alt_for(relative: str) -> tuple[str, str]:
    path = Path(relative)
    stem = path.stem
    lower = relative.lower()
    if "/backgrounds/" in f"/{lower}":
        return BACKGROUND_ALT[stem]
    if "/cg/" in f"/{lower}":
        return CG_ALT[stem]
    if "/ui/" in f"/{lower}":
        return UI_ALT[stem]
    if "/sprites/" in f"/{lower}":
        character = "林瑶" if "lin-yao" in lower else "周衡"
        character_en = "Lin Yao" if "lin-yao" in lower else "Zhou Heng"
        parts = stem.split("-")
        expression = parts[0]
        pose = "-".join(parts[1:])
        return (f"{character}的{expression}表情、{pose}姿态透明全身立绘。", f"Transparent full-body sprite of {character_en}, expression {expression}, pose {pose}.")
    if "/reference/" in f"/{lower}":
        character = "林瑶" if "lin-yao" in lower else "周衡"
        character_en = "Lin Yao" if "lin-yao" in lower else "Zhou Heng"
        return (f"{character}的多角度角色设定与表情参考图。", f"Multi-angle character turnaround and expression reference for {character_en}.")
    if "/keyart/key-art" in f"/{lower}":
        return ("林瑶与周衡站在雨夜玻璃都市前，洋红信号横贯二人身后的主视觉。", "Key art of Lin Yao and Zhou Heng before a rainy glass metropolis crossed by a magenta signal.")
    if "/keyart/title-background" in f"/{lower}":
        return ("雨水覆盖的公寓窗外是一座深蓝城市，桌上信号仪旁留出标题空间。", "A deep-blue city through rain-covered apartment glass, with a signal analyzer and open title space.")
    if "/endings/" in f"/{lower}":
        return CG_ALT[f"ending-{stem}"]
    if "/gallery/thumbnails/" in f"/{lower}":
        return (f"画廊缩略图：{stem}。", f"Gallery thumbnail: {stem}.")
    if "/video/source/" in f"/{lower}":
        return (f"雨幕信号循环视频的可重建分层源：{stem}。", f"Rebuildable source layer for the rain-signal loop: {stem}.")
    return (f"视觉素材：{stem}。", f"Visual asset: {stem}.")


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def build(pack: Path) -> None:
    visual_paths = sorted(path for path in (pack / "Visual").rglob("*.png") if ".local" not in path.parts)
    alt_items = []
    prompt_records = []
    for path in visual_paths:
        relative = path.relative_to(pack).as_posix()
        zh, en = alt_for(relative)
        alt_items.append({"id": safe_id(relative), "relative_path": relative, "alt": {"zh_hans": zh, "en": en}})
        if "/Gallery/Thumbnails/" not in relative and "/Video/Source/" not in relative:
            prompt_records.append({
                "id": f"prompt.{safe_id(relative)[6:]}",
                "artifact": relative,
                "prompt_family": "nativevn-rain-glass-production",
                "prompt_parameters": {"subject": path.stem, "palette": "deep-navy-cyan-magenta", "text": "forbidden"},
                "reference_assets": [value for value in ["Visual/Characters/Reference/lin-yao-turnaround.png" if "lin-yao" in relative or "/CG/" in relative or "/Endings/" in relative or "/KeyArt/key-art" in relative else None, "Visual/Characters/Reference/zhou-heng-turnaround.png" if "zhou-heng" in relative or "/CG/" in relative or "/Endings/" in relative or "/KeyArt/key-art" in relative else None] if value],
                "model": "openai-imagegen",
                "selected": True,
                "sha256": sha256(path),
            })
    write_json(pack / "Visual/alt-text.json", {"schema": "nativevn.flagship_alt_text.v1", "id": "nativevn-flagship-alt-text", "items": alt_items})
    write_json(pack / "Visual/prompts.json", {
        "schema": "nativevn.flagship_visual_prompts.v1",
        "id": "nativevn-flagship-visual-prompts",
        "records": prompt_records,
        "rejected": [{"private_alias": "private://generation/key-art-duplicate-figures.png", "sha256": "83bc995c285519458098cfe6d5039d4115e0e891b0300c62c18be4036a8a82d9", "reason": "duplicate_background_figures"}, {"private_alias": "private://generation/batch-network-failure", "reason": "network_request_failed_no_artifact"}],
    })

    excluded = {"content-manifest.json", "provenance.json", "review.json"}
    private_parts = {".local", ".astra-cache", "__pycache__"}
    public_files = sorted(
        path
        for path in pack.rglob("*")
        if path.is_file()
        and private_parts.isdisjoint(path.parts)
        and not (path.parent == pack / "Manifests" and path.name in excluded)
    )
    assets = []
    provenance = []
    for path in public_files:
        relative = path.relative_to(pack).as_posix()
        media = media_info(path)
        generation = generation_for(relative)
        identifier = safe_id(relative)
        role = role_for(relative)
        license_status = "user_authorized" if role == "voice" else "approved"
        asset = {"id": identifier, "role": role, "locator": {"relative_path": relative}, "sha256": sha256(path), "byte_size": path.stat().st_size, "media": media, "generation_source": generation, "license_status": license_status, "license_evidence": {"relative_path": "STATUS.md" if role == "voice" else "README.md"}, "release_eligible": True}
        assets.append(asset)
        provenance.append({"id": f"prov.{identifier[6:]}"[:128].rstrip("-."), "role": "story_reference" if asset["role"] in {"manifest", "review_evidence"} else asset["role"], "artifact": {"relative_path": relative}, "scene_id": "global", "route_id": "route_common", "line_id": "global", "sha256": asset["sha256"], "byte_size": asset["byte_size"], "media_attributes": {key: value for key, value in media.items() if key in {"kind", "mime_type", "width", "height", "duration_ms", "sample_rate_hz", "channels"}}, "generation_source": generation, "license": {"status": license_status, "scope": "public_redistribution", "evidence": {"relative_path": "STATUS.md" if role == "voice" else "README.md"}}, "release_eligible": True})
    write_json(pack / "Manifests/content-manifest.json", {"schema": "nativevn.flagship_content_manifest.v1", "package_id": "com.astra.nativevn.signal-glass-rain", "title": {"zh_hans": "玻璃雨中的信号", "en": "Signal in the Glass Rain"}, "status": {"content_creation": "complete", "public_release_assets": "ready_with_authorized_voice", "engine_integration": "cook_ready_with_voice", "stage_gate": "IN_PROGRESS"}, "schema_refs": SCHEMA_REFS, "assets": assets})
    write_json(pack / "Manifests/provenance.json", {"schema": "nativevn.flagship_provenance.v1", "package_id": "com.astra.nativevn.signal-glass-rain", "records": provenance})

    review_specs = [
        ("content", "content_creation", "Localization/editorial-review.md", "pass", "not_applicable", True, []),
        ("visual", "visual_spec", "Visual/KeyArt/key-art.png", "pass", "approved", True, []),
        ("ui", "ui_tokens", "Design/ui-tokens.md", "pass", "not_applicable", True, []),
        ("accessibility", "alt_text", "Visual/alt-text.json", "pass", "not_applicable", True, []),
        ("audio_model", "audio_model_review", "Audio/openrouter-audio-review.json", "pass", "not_applicable", True, []),
        ("license", "license", "Audio/audio-qa-report.json", "blocked", "approved", False, ["manual_listening_pending"]),
        ("voice", "voice_rights", "Manifests/voice-release.json", "pass", "user_authorized", True, []),
        ("release", "release_readiness", "STATUS.md", "blocked", "approved", False, ["runtime_player_evidence_pending"]),
    ]
    reviews = []
    for role, review_type, relative, decision, license_status, eligible, findings in review_specs:
        path = pack / relative
        media = media_info(path)
        reviews.append({"id": f"review.{role}", "subject_id": safe_id(relative), "role": role, "artifact": {"relative_path": relative}, "scene_id": "global", "route_id": "route_common", "line_id": "global", "sha256": sha256(path), "byte_size": path.stat().st_size, "media": media, "generation_source": {"kind": "hand_authored", "method": "manual-review", "tool": "codex"}, "review_type": review_type, "reviewer_alias": "content.qa", "reviewed_on": "2026-07-15", "decision": decision, "license_status": license_status, "release_eligible": eligible, "finding_codes": findings})
    write_json(pack / "Manifests/review.json", {"schema": "nativevn.flagship_review.v1", "package_id": "com.astra.nativevn.signal-glass-rain", "reviews": reviews})


def main(argv: list[str] | None = None) -> int:
    repo = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pack-root", type=Path, default=repo / "Examples/NativeVN")
    args = parser.parse_args(argv)
    try:
        build(args.pack_root.resolve())
    except (OSError, KeyError, ValueError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"NATIVEVN_METADATA_BUILD_FAILED: {type(error).__name__}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
