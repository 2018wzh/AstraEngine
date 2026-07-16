#!/usr/bin/env python3
"""Validate NativeVN flagship content integrity, safety, media, and release readiness."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import struct
import subprocess
import sys
import wave
import zlib
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

from common import Diagnostics, SAFE_ID, ToolFailure, display_path, iter_files, load_json, sha256_file


TEXT_EXTENSIONS = {".json", ".md", ".txt", ".yaml", ".yml", ".toml", ".csv", ".py"}
ABSOLUTE_PATH_PATTERNS = (
    re.compile(r"(?<![A-Za-z0-9_])[A-Za-z]:[\\/](?:[^\s\"'<>]+)"),
    re.compile(r"(?<![:A-Za-z0-9_])/(?:Users|home|mnt|Volumes|var/tmp|private/tmp)/[^\s\"'<>]+"),
)
SECRET_PATTERNS = (
    re.compile(r"\bsk-[A-Za-z0-9_-]{20,}\b"),
    re.compile(r"\bxi-[A-Za-z0-9_-]{20,}\b"),
    re.compile(r"ELEVENLABS_API_KEY\s*[:=]\s*[\"']?(?!\$\{|<|REDACTED|YOUR_)[A-Za-z0-9_-]{16,}", re.IGNORECASE),
)
LOCALIZED_FIELDS = {"title", "name", "text", "summary", "description", "label"}
REFERENCE_FIELDS = {"cue_id", "voice_cue_id", "route_id", "next_route_id", "target_route_id"}


@dataclass(frozen=True)
class PngInfo:
    width: int
    height: int
    color_type: int
    has_alpha: bool
    transparent_pixels: int
    opaque_pixels: int
    chroma_pixels: int


@dataclass(frozen=True)
class WavInfo:
    duration_seconds: float
    sample_rate: int
    sample_width_bits: int
    channels: int
    peak: float
    rms_dbfs: float
    clipped_samples: int


class ContentPackValidator:
    def __init__(self, root: Path, *, release: bool = False) -> None:
        self.root = root.resolve()
        self.release = release
        self.diagnostics = Diagnostics()
        self.counts = {"files": 0, "json": 0, "images": 0, "wav": 0, "ogg": 0, "manifest_entries": 0}
        self.route_ids: set[str] = set()
        self.cue_ids: set[str] = set()
        self.references: list[tuple[str, str, str]] = []

    def relative(self, path: Path) -> str:
        return display_path(path, self.root)

    def missing(self, code: str, message: str, path: str) -> None:
        if self.release:
            self.diagnostics.error(code, message, path)
        else:
            self.diagnostics.warning(code, message, path)

    def validate(self) -> tuple[Diagnostics, dict[str, int | bool]]:
        if not self.root.is_dir():
            self.diagnostics.error("NATIVEVN_PACK_ROOT_MISSING", "content-pack root does not exist", ".")
            return self.diagnostics, self.summary()
        files = list(iter_files(self.root, excluded_parts={"__pycache__", ".astra-cache"}))
        self.counts["files"] = len(files)
        self._validate_required_shape(files)
        self._validate_project_shape(files)
        self._validate_text_safety(files)
        self._validate_json_files(files)
        self._validate_references()
        self._validate_narrative_alignment()
        self._validate_release_voice()
        self._validate_images(files)
        self._validate_audio(files)
        self._validate_audio_qa()
        self._validate_openrouter_audio_review()
        self._validate_completed_inventory(files)
        self._validate_git_private_voice()
        return self.diagnostics, self.summary()

    def summary(self) -> dict[str, int | bool]:
        return {**self.counts, "release_mode": self.release}

    def _validate_required_shape(self, files: list[Path]) -> None:
        required = [
            (self.root / "Audio" / "audio-manifest.json", "NATIVEVN_AUDIO_MANIFEST_MISSING"),
            (self.root / "Audio" / "audio-qa-report.json", "NATIVEVN_AUDIO_QA_MISSING"),
            (self.root / "Audio" / "openrouter-audio-review.json", "NATIVEVN_OPENROUTER_AUDIO_REVIEW_MISSING"),
            (self.root / "Narrative" / "voice-cues.json", "NATIVEVN_VOICE_CUES_MISSING"),
            (self.root / "Manifests" / "content-manifest.json", "NATIVEVN_CONTENT_MANIFEST_MISSING"),
            (self.root / "Manifests" / "provenance.json", "NATIVEVN_PROVENANCE_MISSING"),
            (self.root / "Manifests" / "review.json", "NATIVEVN_REVIEW_MISSING"),
            (self.root / "Visual" / "alt-text.json", "NATIVEVN_ALT_TEXT_MISSING"),
            (self.root / "Visual" / "prompts.json", "NATIVEVN_PROMPTS_MISSING"),
            (self.root / "Visual" / "Video" / "Source" / "rebuild.json", "NATIVEVN_VIDEO_SOURCE_MISSING"),
            (self.root / "Visual" / "Video" / "rain-signal-loop.mp4", "NATIVEVN_VIDEO_MP4_MISSING"),
            (self.root / "Visual" / "Video" / "rain-signal-loop.webm", "NATIVEVN_VIDEO_WEBM_MISSING"),
            (self.root / "project.yaml", "NATIVEVN_PROJECT_DESCRIPTOR_MISSING"),
            (self.root / "Scripts" / "main.astra", "NATIVEVN_STORY_SOURCE_MISSING"),
            (self.root / "UI" / "flagship.astra", "NATIVEVN_UI_SOURCE_MISSING"),
            (self.root / "Localization" / "zh-Hans.json", "NATIVEVN_RUNTIME_LOCALIZATION_MISSING"),
            (self.root / "Localization" / "en.json", "NATIVEVN_RUNTIME_LOCALIZATION_MISSING"),
            (self.root / "Manifests" / "voice-release.json", "NATIVEVN_RELEASE_VOICE_MANIFEST_MISSING"),
        ]
        for path, code in required:
            if not path.is_file():
                self.missing(code, "required flagship content file is not present", self.relative(path))
        if not any(path.suffix.lower() == ".png" and ".local" not in path.parts for path in files):
            self.missing("NATIVEVN_VISUAL_ASSETS_MISSING", "no public PNG visual assets are present", "Visual")
        if not any(path.suffix.lower() == ".json" and "route" in path.name.lower() for path in files):
            self.missing("NATIVEVN_ROUTE_DATA_MISSING", "no route JSON data is present", "Narrative")
        for path in files:
            if ".zh-hans." not in path.name.lower():
                continue
            counterpart_name = path.name.lower().replace(".zh-hans.", ".en.")
            counterpart_exists = any(
                candidate.name.lower() == counterpart_name
                and candidate.parent in {path.parent, self.root / "Localization"}
                for candidate in files
            )
            if not counterpart_exists:
                self.missing(
                    "NATIVEVN_BILINGUAL_COUNTERPART_MISSING",
                    "Chinese narrative JSON requires an English counterpart in Narrative or Localization",
                    self.relative(path),
                )
    def _validate_project_shape(self, files: list[Path]) -> None:
        scenarios = [path for path in files if "scenario" in path.name.lower()]
        if scenarios:
            self.diagnostics.error(
                "NATIVEVN_RUNTIME_TEST_SCOPE_FORBIDDEN",
                "the cook-only milestone must not add runtime scenario inputs",
                self.relative(scenarios[0]),
            )
        story_path = self.root / "Scripts" / "main.astra"
        if story_path.is_file():
            story = story_path.read_text(encoding="utf-8")
            if story.count("#@id line.") != 180 or story.count("      option key:") != 3:
                self.diagnostics.error(
                    "NATIVEVN_PROJECT_STORY_COVERAGE_INVALID",
                    "runtime story must bind all 180 lines and exactly three route options",
                    self.relative(story_path),
                )
            if story.count("    voice asset:") != 180 or story.count(" voice:voice.") != 180:
                self.diagnostics.error(
                    "NATIVEVN_PROJECT_VOICE_COVERAGE_INVALID",
                    "release story must bind exactly one voice command to every canonical line",
                    self.relative(story_path),
                )
        sidecars = [path for path in files if path.name.endswith(".astra-asset.yaml")]
        if len(sidecars) < 280:
            self.missing(
                "NATIVEVN_PROJECT_ASSET_SIDECARS_INCOMPLETE",
                "the flagship project must bind the completed visual, audio, video, and font inventory",
                "AssetSidecars",
            )
        voice_sidecars = [path for path in sidecars if path.name.startswith("voice__")]
        if len(voice_sidecars) != 180:
            self.missing(
                "NATIVEVN_PROJECT_VOICE_SIDECARS_INVALID",
                "release project must contain exactly 180 voice asset sidecars",
                "AssetSidecars",
            )

    def _validate_text_safety(self, files: list[Path]) -> None:
        for path in files:
            if path.suffix.lower() not in TEXT_EXTENSIONS or path.stat().st_size > 8 * 1024 * 1024:
                continue
            try:
                text = path.read_text(encoding="utf-8")
            except (OSError, UnicodeError):
                self.diagnostics.error("NATIVEVN_TEXT_ENCODING_INVALID", "text file must be readable UTF-8", self.relative(path))
                continue
            for pattern in ABSOLUTE_PATH_PATTERNS:
                if pattern.search(text):
                    self.diagnostics.error("NATIVEVN_ABSOLUTE_PATH_FORBIDDEN", "text content contains a local absolute path", self.relative(path))
                    break
            for pattern in SECRET_PATTERNS:
                if pattern.search(text):
                    self.diagnostics.error("NATIVEVN_SECRET_FORBIDDEN", "text content appears to contain a secret", self.relative(path))
                    break

    def _validate_json_files(self, files: list[Path]) -> None:
        for path in files:
            if path.suffix.lower() != ".json":
                continue
            self.counts["json"] += 1
            relative = self.relative(path)
            try:
                payload = load_json(path)
            except ToolFailure as error:
                self.diagnostics.error(error.code, error.message, relative)
                continue
            if not isinstance(payload, dict):
                self.diagnostics.error("NATIVEVN_JSON_ROOT_INVALID", "JSON root must be an object", relative)
                continue
            if "$schema" in payload or path.name.endswith(".schema.json"):
                self._validate_json_schema(payload, relative)
                continue
            schema = payload.get("schema")
            identifier = payload.get("id") or payload.get("story_id") or payload.get("package_id")
            runtime_data_schema = schema in {
                "astra.vn.localization_table.v1",
                "astra.ui_theme_manifest.v1",
            }
            if runtime_data_schema:
                self._walk_json(payload, relative, ())
                continue
            valid_schema_prefix = isinstance(schema, str) and schema.startswith(("astra.nativevn.flagship.", "astra.nativevn_flagship.", "nativevn.flagship_"))
            if not valid_schema_prefix or not schema.endswith(".v1"):
                self.diagnostics.error("NATIVEVN_JSON_SCHEMA_INVALID", "JSON must declare a flagship v1 schema", relative)
            if not isinstance(identifier, str) or SAFE_ID.fullmatch(identifier) is None:
                self.diagnostics.error("NATIVEVN_JSON_ID_INVALID", "JSON must declare a lowercase safe id", relative)
            self._walk_json(payload, relative, ())
            self._collect_definitions(payload, relative)
            if "manifest" in path.name.lower() or isinstance(payload.get("assets"), list) or isinstance(payload.get("files"), list):
                self._validate_manifest(payload, path)

    def _validate_json_schema(self, payload: dict[str, Any], relative: str) -> None:
        dialect = payload.get("$schema")
        schema_id = payload.get("$id")
        if dialect != "https://json-schema.org/draft/2020-12/schema":
            self.diagnostics.error("NATIVEVN_JSON_SCHEMA_DIALECT_INVALID", "JSON Schema must use draft 2020-12", relative)
        if not isinstance(schema_id, str) or not schema_id.startswith("https://astra.invalid/schema/nativevn.flagship"):
            self.diagnostics.error("NATIVEVN_JSON_SCHEMA_ID_INVALID", "JSON Schema must use the stable Astra schema id namespace", relative)
        if payload.get("type") != "object" or not isinstance(payload.get("title"), str) or not payload["title"].strip():
            self.diagnostics.error("NATIVEVN_JSON_SCHEMA_ROOT_INVALID", "JSON Schema must describe a titled object root", relative)

    def _walk_json(self, value: Any, relative: str, trail: tuple[str, ...]) -> None:
        if isinstance(value, dict):
            for key, child in value.items():
                next_trail = (*trail, key)
                if key in LOCALIZED_FIELDS and isinstance(child, dict):
                    zh_value = child.get("zh") or child.get("zh_hans") or child.get("zh-Hans")
                    if not isinstance(zh_value, str) or not zh_value.strip() or not isinstance(child.get("en"), str) or not child["en"].strip():
                        self.diagnostics.error("NATIVEVN_BILINGUAL_FIELD_INVALID", f"localized field '{key}' must contain non-empty Chinese and en strings", relative)
                if key in REFERENCE_FIELDS and isinstance(child, str):
                    reference_kind = "cue" if "cue" in key else "route"
                    self.references.append((reference_kind, child, relative))
                external_identifier_fields = {"model_id", "voice_id"}
                if key.endswith("_id") and key not in external_identifier_fields and isinstance(child, str) and SAFE_ID.fullmatch(child) is None:
                    self.diagnostics.error("NATIVEVN_REFERENCE_ID_INVALID", f"field '{key}' contains an unsafe identifier", relative)
                self._walk_json(child, relative, next_trail)
        elif isinstance(value, list):
            for index, child in enumerate(value):
                self._walk_json(child, relative, (*trail, str(index)))

    def _collect_definitions(self, payload: dict[str, Any], relative: str) -> None:
        for collection_name, target in (("routes", self.route_ids), ("cues", self.cue_ids)):
            canonical = {
                "routes": "Narrative/route-graph.json",
                "cues": "Narrative/voice-cues.json",
            }[collection_name]
            if relative.replace("\\", "/") != canonical:
                continue
            collection = payload.get(collection_name)
            if not isinstance(collection, list):
                continue
            local: set[str] = set()
            for item in collection:
                if not isinstance(item, dict):
                    self.diagnostics.error("NATIVEVN_COLLECTION_ITEM_INVALID", f"{collection_name} entries must be objects", relative)
                    continue
                item_id = item.get("id") or item.get(f"{collection_name[:-1]}_id")
                if not isinstance(item_id, str) or SAFE_ID.fullmatch(item_id) is None:
                    self.diagnostics.error("NATIVEVN_COLLECTION_ID_INVALID", f"{collection_name} entries require safe ids", relative)
                    continue
                if item_id in local or item_id in target:
                    self.diagnostics.error("NATIVEVN_COLLECTION_ID_DUPLICATE", f"duplicate {collection_name[:-1]} id", relative)
                local.add(item_id)
                target.add(item_id)

    def _validate_references(self) -> None:
        for kind, identifier, relative in self.references:
            definitions = self.cue_ids if kind == "cue" else self.route_ids
            if definitions and identifier not in definitions:
                self.diagnostics.error("NATIVEVN_REFERENCE_UNRESOLVED", f"{kind} reference does not resolve to a declared id", relative)
        if self.release and not self.route_ids:
            self.diagnostics.error("NATIVEVN_ROUTE_COVERAGE_EMPTY", "release content must declare at least one route", "Narrative")
        if self.release and not self.cue_ids:
            self.diagnostics.error("NATIVEVN_CUE_COVERAGE_EMPTY", "release content must declare at least one cue", "Narrative")

    def _validate_narrative_alignment(self) -> None:
        chinese_path = self.root / "Narrative" / "screenplay.zh-Hans.json"
        english_path = self.root / "Localization" / "screenplay.en.json"
        cues_path = self.root / "Narrative" / "voice-cues.json"
        graph_path = self.root / "Narrative" / "route-graph.json"
        if not all(path.is_file() for path in (chinese_path, english_path, cues_path, graph_path)):
            return
        try:
            chinese = load_json(chinese_path)
            english = load_json(english_path)
            cues = load_json(cues_path)
            graph = load_json(graph_path)
        except ToolFailure:
            return

        def indexed_scenes(payload: dict[str, Any]) -> dict[str, dict[str, Any]]:
            return {scene["id"]: scene for scene in payload.get("scenes", []) if isinstance(scene, dict) and isinstance(scene.get("id"), str)}

        chinese_scenes = indexed_scenes(chinese)
        english_scenes = indexed_scenes(english)
        if chinese_scenes.keys() != english_scenes.keys():
            self.diagnostics.error("NATIVEVN_LOCALIZATION_SCENE_MISMATCH", "Chinese and English scene ids must match exactly", self.relative(english_path))

        chinese_lines: dict[str, dict[str, Any]] = {}
        english_lines: dict[str, dict[str, Any]] = {}
        for scene_id, scene in chinese_scenes.items():
            for line in scene.get("lines", []):
                if isinstance(line, dict) and isinstance(line.get("id"), str):
                    if line["id"] in chinese_lines:
                        self.diagnostics.error("NATIVEVN_LINE_ID_DUPLICATE", "Chinese line id is duplicated", self.relative(chinese_path))
                    chinese_lines[line["id"]] = line
        for scene in english_scenes.values():
            for line in scene.get("lines", []):
                if isinstance(line, dict) and isinstance(line.get("id"), str):
                    if line["id"] in english_lines:
                        self.diagnostics.error("NATIVEVN_LINE_ID_DUPLICATE", "English line id is duplicated", self.relative(english_path))
                    english_lines[line["id"]] = line
        if chinese_lines.keys() != english_lines.keys():
            self.diagnostics.error("NATIVEVN_LOCALIZATION_LINE_MISMATCH", "Chinese and English line ids must match exactly", self.relative(english_path))

        for line_id in chinese_lines.keys() & english_lines.keys():
            expected = chinese_lines[line_id]
            translated = english_lines[line_id]
            for field in ("speaker", "scene_id", "route_scope", "voice_cue_id"):
                if expected.get(field) != translated.get(field):
                    self.diagnostics.error("NATIVEVN_LOCALIZATION_BINDING_MISMATCH", f"localized line field '{field}' must match the canonical line", self.relative(english_path))

        def choice_ids(payload: dict[str, Any]) -> set[str]:
            return {choice["id"] for choice in payload.get("choices", []) if isinstance(choice, dict) and isinstance(choice.get("id"), str)}

        if choice_ids(chinese) != choice_ids(english):
            self.diagnostics.error("NATIVEVN_LOCALIZATION_CHOICE_MISMATCH", "Chinese and English choice ids must match exactly", self.relative(english_path))

        cue_items = [cue for cue in cues.get("cues", []) if isinstance(cue, dict)]
        cue_by_line = {cue.get("line_id"): cue for cue in cue_items if isinstance(cue.get("line_id"), str)}
        if len(cue_by_line) != len(cue_items) or cue_by_line.keys() != chinese_lines.keys():
            self.diagnostics.error("NATIVEVN_VOICE_CUE_ALIGNMENT_MISMATCH", "every canonical line must have exactly one voice cue and no orphan cue", self.relative(cues_path))
        for line_id in cue_by_line.keys() & chinese_lines.keys():
            cue = cue_by_line[line_id]
            line = chinese_lines[line_id]
            if cue.get("id") != line.get("voice_cue_id") or cue.get("speaker_id") != line.get("speaker") or cue.get("scene_id") != line.get("scene_id"):
                self.diagnostics.error("NATIVEVN_VOICE_CUE_BINDING_MISMATCH", "voice cue binding must match line, speaker, and scene", self.relative(cues_path))

        routes = {route.get("id"): route for route in graph.get("routes", []) if isinstance(route, dict) and isinstance(route.get("id"), str)}
        terminal_ids = [terminal.get("id") for terminal in graph.get("terminals", []) if isinstance(terminal, dict)]
        branch_ids = {"route_truth", "route_silence", "route_signal"}
        if len(terminal_ids) != 3 or len(set(terminal_ids)) != 3 or not branch_ids.issubset(routes):
            self.diagnostics.error("NATIVEVN_TERMINAL_TOPOLOGY_INVALID", "route graph must contain truth, silence, and signal with three unique terminals", self.relative(graph_path))
        common = routes.get("route_common", {})
        targets = {
            option.get("target_route_id")
            for choice in graph.get("choices", []) if isinstance(choice, dict)
            for option in choice.get("options", []) if isinstance(option, dict)
        }
        if not branch_ids.issubset(targets) or not common.get("next_choice_id"):
            self.diagnostics.error("NATIVEVN_ROUTE_REACHABILITY_INVALID", "common route choice must reach every ending route", self.relative(graph_path))

        duration_by_scene = {
            scene_id: sum(line.get("estimated_seconds", 0) for line in scene.get("lines", []) if isinstance(line, dict))
            for scene_id, scene in chinese_scenes.items()
        }
        common_seconds = sum(duration_by_scene.get(scene_id, 0) for scene_id in common.get("scene_ids", []))
        if not 480 <= common_seconds <= 600:
            self.diagnostics.error("NATIVEVN_COMMON_DURATION_OUT_OF_RANGE", "common route must be 8 to 10 minutes", self.relative(chinese_path))
        for route_id in branch_ids:
            route = routes.get(route_id, {})
            branch_seconds = sum(duration_by_scene.get(scene_id, 0) for scene_id in route.get("scene_ids", []))
            total_seconds = common_seconds + branch_seconds
            if not 360 <= branch_seconds <= 480 or not 900 <= total_seconds <= 1200:
                self.diagnostics.error("NATIVEVN_ROUTE_DURATION_OUT_OF_RANGE", "each branch must be 6 to 8 minutes and a playthrough 15 to 20 minutes", self.relative(chinese_path))

    def _validate_release_voice(self) -> None:
        manifest_path = self.root / "Manifests" / "voice-release.json"
        screenplay_path = self.root / "Narrative" / "screenplay.zh-Hans.json"
        if not manifest_path.is_file() or not screenplay_path.is_file():
            return
        try:
            manifest = load_json(manifest_path)
            screenplay = load_json(screenplay_path)
        except ToolFailure:
            return
        expected = {
            line["voice_cue_id"]: line
            for scene in screenplay.get("scenes", [])
            for line in scene.get("lines", [])
            if isinstance(line, dict) and isinstance(line.get("voice_cue_id"), str)
        }
        cues = manifest.get("cues", [])
        actual = {cue.get("id"): cue for cue in cues if isinstance(cue, dict)}
        if manifest.get("status") != "release_ready" or manifest.get("cue_count") != 180 or len(cues) != 180 or actual.keys() != expected.keys():
            self.diagnostics.error("NATIVEVN_RELEASE_VOICE_COVERAGE_INVALID", "release voice manifest must cover all 180 canonical cues exactly once", self.relative(manifest_path))
            return
        for cue_id, cue in actual.items():
            line = expected[cue_id]
            if cue.get("line_id") != line.get("id") or cue.get("speaker_id") != line.get("speaker") or cue.get("license_status") != "user_authorized" or cue.get("release_eligible") is not True:
                self.diagnostics.error("NATIVEVN_RELEASE_VOICE_BINDING_INVALID", "release voice cue binding or authorization is invalid", self.relative(manifest_path))
                continue
            for field in ("asset", "master"):
                locator = cue.get(field, {})
                relative = locator.get("relative_path") if isinstance(locator, dict) else None
                path = self.root / relative if isinstance(relative, str) else None
                if path is None or not path.is_file():
                    self.diagnostics.error("NATIVEVN_RELEASE_VOICE_FILE_MISSING", "release voice file is missing", self.relative(manifest_path))
            asset_relative = cue.get("asset", {}).get("relative_path")
            asset = self.root / asset_relative if isinstance(asset_relative, str) else None
            if asset is not None and asset.is_file() and (cue.get("sha256") != sha256_file(asset) or cue.get("byte_size") != asset.stat().st_size):
                self.diagnostics.error("NATIVEVN_RELEASE_VOICE_HASH_MISMATCH", "release voice hash or byte size is invalid", self.relative(manifest_path))

    def _validate_manifest(self, payload: dict[str, Any], manifest_path: Path) -> None:
        relative = self.relative(manifest_path)
        entries: list[Any] = []
        for key in ("assets", "files"):
            value = payload.get(key)
            if isinstance(value, list):
                entries.extend(value)
        seen_paths: set[str] = set()
        seen_ids: set[str] = set()
        for entry in entries:
            if not isinstance(entry, dict):
                self.diagnostics.error("NATIVEVN_MANIFEST_ENTRY_INVALID", "manifest entries must be objects", relative)
                continue
            entry_id = entry.get("id")
            if isinstance(entry_id, str):
                if entry_id in seen_ids:
                    self.diagnostics.error("NATIVEVN_MANIFEST_ID_DUPLICATE", "manifest asset ids must be unique", relative)
                seen_ids.add(entry_id)
            records = [entry]
            records.extend(value for key in ("master", "distribution") if isinstance((value := entry.get(key)), dict))
            for record in records:
                path_value = record.get("path")
                path_base = manifest_path.parent
                locator = record.get("locator") or record.get("asset") or record.get("artifact")
                if not isinstance(path_value, str) and isinstance(locator, dict):
                    private_alias = locator.get("private_alias")
                    if isinstance(private_alias, str):
                        if record.get("release_eligible") is True:
                            self.diagnostics.error("NATIVEVN_PRIVATE_ASSET_RELEASE_FORBIDDEN", "private alias cannot be release eligible", relative)
                        continue
                    path_value = locator.get("relative_path")
                    path_base = self.root
                if not isinstance(path_value, str):
                    continue
                normalized = Path(path_value)
                if normalized.is_absolute() or ".." in normalized.parts:
                    self.diagnostics.error("NATIVEVN_MANIFEST_PATH_INVALID", "manifest paths must be pack-relative and contained", relative)
                    continue
                asset_path = (path_base / normalized).resolve()
                try:
                    asset_path.relative_to(self.root)
                except ValueError:
                    self.diagnostics.error("NATIVEVN_MANIFEST_PATH_INVALID", "manifest path escapes the content pack", relative)
                    continue
                canonical = self.relative(asset_path)
                if canonical in seen_paths:
                    self.diagnostics.error("NATIVEVN_MANIFEST_PATH_DUPLICATE", "manifest paths must be unique", relative)
                seen_paths.add(canonical)
                if not asset_path.is_file():
                    self.diagnostics.error("NATIVEVN_MANIFEST_FILE_MISSING", "manifest references a missing file", canonical)
                    continue
                expected_size = record.get("byte_size")
                expected_hash = record.get("sha256")
                if not isinstance(expected_size, int) or expected_size != asset_path.stat().st_size:
                    self.diagnostics.error("NATIVEVN_MANIFEST_SIZE_MISMATCH", "manifest byte_size does not match the file", canonical)
                if not isinstance(expected_hash, str) or expected_hash != sha256_file(asset_path):
                    self.diagnostics.error("NATIVEVN_MANIFEST_HASH_MISMATCH", "manifest sha256 does not match the file", canonical)
                self.counts["manifest_entries"] += 1
        declared_count = payload.get("asset_count")
        if isinstance(declared_count, int) and declared_count != len(payload.get("assets", [])):
            self.diagnostics.error("NATIVEVN_MANIFEST_COUNT_MISMATCH", "manifest asset_count is inconsistent", relative)

    def _validate_images(self, files: list[Path]) -> None:
        for path in files:
            if path.suffix.lower() != ".png" or ".local" in path.parts:
                continue
            self.counts["images"] += 1
            relative = self.relative(path)
            try:
                info = inspect_png(path)
            except ToolFailure as error:
                self.diagnostics.error(error.code, error.message, relative)
                continue
            lowered = relative.lower()
            if any(token in lowered for token in ("background", "backdrop", "/bg/")):
                if info.width < 1280 or info.height < 720:
                    self.diagnostics.error("NATIVEVN_BACKGROUND_DIMENSIONS_INVALID", "background image must be at least 1280x720", relative)
            if any(token in lowered for token in ("character", "portrait", "sprite")) and "/reference/" not in lowered:
                if not info.has_alpha or info.transparent_pixels == 0:
                    self.diagnostics.error("NATIVEVN_IMAGE_ALPHA_REQUIRED", "character sprite image must contain transparency", relative)
            total_visible = info.opaque_pixels + info.chroma_pixels
            if total_visible and info.chroma_pixels / total_visible > 0.20:
                self.diagnostics.error("NATIVEVN_IMAGE_CHROMA_SUSPECT", "image contains excessive key-green/key-magenta pixels", relative)

    def _validate_audio(self, files: list[Path]) -> None:
        for path in files:
            suffix = path.suffix.lower()
            relative = self.relative(path)
            if suffix == ".wav" and ".local" not in path.parts:
                self.counts["wav"] += 1
                try:
                    info = inspect_wav(path)
                except ToolFailure as error:
                    self.diagnostics.error(error.code, error.message, relative)
                    continue
                expected_channels = 1 if relative.startswith("Audio/Voice/Master/") else 2
                if (info.sample_rate, info.sample_width_bits, info.channels) != (48_000, 24, expected_channels):
                    channel_label = "mono" if expected_channels == 1 else "stereo"
                    self.diagnostics.error("NATIVEVN_WAV_FORMAT_INVALID", f"WAV must be 48kHz 24-bit {channel_label} PCM", relative)
                if info.clipped_samples:
                    self.diagnostics.error("NATIVEVN_WAV_CLIPPING", "WAV contains full-scale clipped samples", relative)
                if info.rms_dbfs < -58.0 or info.peak < 0.001:
                    self.diagnostics.error("NATIVEVN_WAV_SILENT", "WAV is silent or below the minimum programme level", relative)
                kind = path.parent.name.lower()
                bounds = {"bgm": (60.0, 90.0), "stinger": (5.0, 9.0), "se": (0.2, 8.0)}.get(kind)
                if bounds is not None and not bounds[0] <= info.duration_seconds <= bounds[1]:
                    self.diagnostics.error("NATIVEVN_AUDIO_DURATION_INVALID", f"{kind} duration is outside its contract", relative)
            elif suffix == ".ogg" and ".local" not in path.parts:
                self.counts["ogg"] += 1
                try:
                    magic = path.read_bytes()[:4]
                except OSError:
                    magic = b""
                if magic != b"OggS":
                    self.diagnostics.error("NATIVEVN_OGG_CONTAINER_INVALID", "distribution audio must be a real OGG container", relative)
        if self.release and (self.counts["wav"] != 205 or self.counts["ogg"] != 205):
            self.diagnostics.error("NATIVEVN_AUDIO_CATALOG_INCOMPLETE", "release project requires 25 music/SE plus 180 voice WAV masters and OGG distributions", "Audio")

    def _validate_audio_qa(self) -> None:
        path = self.root / "Audio" / "audio-qa-report.json"
        if not path.is_file():
            return
        try:
            report = load_json(path)
        except ToolFailure:
            return
        assets = report.get("assets", [])
        if report.get("asset_count") != 25 or len(assets) != 25 or report.get("automated_decision") != "pass":
            self.diagnostics.error("NATIVEVN_AUDIO_QA_INVALID", "audio QA must cover all 25 assets with an automated pass", self.relative(path))
        if any(asset.get("automated_decision") != "pass" or asset.get("finding_codes") for asset in assets if isinstance(asset, dict)):
            self.diagnostics.error("NATIVEVN_AUDIO_QA_FINDING", "audio QA contains an unresolved automated finding", self.relative(path))
        if report.get("manual_listening", {}).get("status") != "pass":
            if self.release:
                self.diagnostics.error("NATIVEVN_AUDIO_MANUAL_REVIEW_REQUIRED", "release validation requires a completed full manual listening review", self.relative(path))
            else:
                self.diagnostics.warning("NATIVEVN_AUDIO_MANUAL_REVIEW_PENDING", "full manual listening remains a release review action", self.relative(path))

    def _validate_openrouter_audio_review(self) -> None:
        path = self.root / "Audio" / "openrouter-audio-review.json"
        manifest_path = self.root / "Audio" / "audio-manifest.json"
        if not path.is_file() or not manifest_path.is_file():
            return
        try:
            report = load_json(path)
            manifest = load_json(manifest_path)
        except ToolFailure:
            return
        assets = report.get("assets", [])
        expected = {asset["id"]: asset for asset in manifest.get("assets", []) if isinstance(asset, dict) and isinstance(asset.get("id"), str)}
        actual = {asset.get("id"): asset for asset in assets if isinstance(asset, dict) and isinstance(asset.get("id"), str)}
        if report.get("asset_count") != 25 or len(assets) != 25 or actual.keys() != expected.keys():
            self.diagnostics.error("NATIVEVN_OPENROUTER_REVIEW_COVERAGE_INVALID", "OpenRouter review must cover every public audio asset exactly once", self.relative(path))
        request = report.get("request", {})
        selection = report.get("selection", {})
        if (
            request.get("requested_model") != "xiaomi/mimo-v2.5"
            or request.get("temperature") != 0
            or request.get("seed_base") != 20_260_715
            or request.get("response_format") != "json_object"
            or selection.get("selected_model") != request.get("requested_model")
        ):
            self.diagnostics.error("NATIVEVN_OPENROUTER_REVIEW_SETTINGS_INVALID", "OpenRouter review settings or selected-model evidence do not match the approved deterministic profile", self.relative(path))
        for asset_id, item in actual.items():
            source = self.root / "Audio" / expected[asset_id]["distribution"]["path"]
            if not source.is_file() or item.get("source_sha256") != sha256_file(source):
                self.diagnostics.error("NATIVEVN_OPENROUTER_REVIEW_SOURCE_MISMATCH", "model review source hash does not match the public distribution asset", self.relative(path))
            if item.get("decision") != "pass" or item.get("fit_for_role") is not True or item.get("defects"):
                self.diagnostics.error("NATIVEVN_OPENROUTER_REVIEW_BLOCKED", "model-assisted listening review contains an unresolved finding", self.relative(path))
            if item.get("selected_model") != request.get("requested_model") or not isinstance(item.get("selected_provider"), str):
                self.diagnostics.error("NATIVEVN_OPENROUTER_REVIEW_ROUTE_INVALID", "selected model/provider evidence is incomplete", self.relative(path))
            normalized = {
                "decision": item.get("decision"),
                "defects": item.get("defects"),
                "summary": item.get("summary"),
                "fit_for_role": item.get("fit_for_role"),
                "contract_normalizations": item.get("contract_normalizations"),
            }
            normalized_hash = hashlib.sha256(json.dumps(normalized, ensure_ascii=False, sort_keys=True).encode("utf-8")).hexdigest()
            if item.get("response_sha256") != normalized_hash:
                self.diagnostics.error("NATIVEVN_OPENROUTER_REVIEW_HASH_MISMATCH", "normalized model response hash is invalid", self.relative(path))
        if report.get("decision") != "pass" or report.get("human_review_replaced") is not False:
            self.diagnostics.error("NATIVEVN_OPENROUTER_REVIEW_STATUS_INVALID", "model review must pass without claiming to replace human review", self.relative(path))

    def _validate_completed_inventory(self, files: list[Path]) -> None:
        manifest_path = self.root / "Manifests" / "content-manifest.json"
        if not manifest_path.is_file():
            return
        try:
            manifest = load_json(manifest_path)
        except ToolFailure:
            return
        if manifest.get("status", {}).get("content_creation") != "complete":
            return

        def png_count(relative_directory: str) -> int:
            directory = self.root / Path(relative_directory)
            return sum(1 for path in files if path.suffix.lower() == ".png" and path.parent == directory)

        expected_counts = {
            "Visual/Characters/Sprites/lin-yao": 18,
            "Visual/Characters/Sprites/zhou-heng": 18,
            "Visual/Characters/Reference": 2,
            "Visual/Backgrounds": 8,
            "Visual/CG": 6,
            "Visual/KeyArt": 2,
            "Visual/Endings": 3,
            "Visual/UI": 9,
            "Visual/Gallery/Thumbnails": 10,
        }
        for directory, expected in expected_counts.items():
            actual = png_count(directory)
            if actual != expected:
                self.diagnostics.error("NATIVEVN_VISUAL_INVENTORY_MISMATCH", f"completed content requires {expected} PNG files in this category, found {actual}", directory)
        if self.counts["images"] != 79:
            self.diagnostics.error("NATIVEVN_VISUAL_TOTAL_MISMATCH", "completed content requires exactly 79 public PNG files including video source layers", "Visual")
        if self.counts["wav"] != 205 or self.counts["ogg"] != 205:
            self.diagnostics.error("NATIVEVN_AUDIO_CATALOG_INCOMPLETE", "completed release content requires 25 music/SE plus 180 voice WAV masters and OGG distributions", "Audio")

        for extension in ("mp4", "webm"):
            path = self.root / "Visual" / "Video" / f"rain-signal-loop.{extension}"
            if not path.is_file():
                continue
            result = subprocess.run(
                ["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,r_frame_rate:format=duration", "-of", "json", str(path)],
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                check=False,
            )
            try:
                probe = json.loads(result.stdout) if result.returncode == 0 else {}
                stream = probe["streams"][0]
                numerator, denominator = (int(part) for part in stream["r_frame_rate"].split("/"))
                frame_rate = numerator / denominator
                duration = float(probe["format"]["duration"])
                valid = int(stream["width"]) == 1920 and int(stream["height"]) == 1080 and abs(frame_rate - 24.0) < 0.001 and abs(duration - 12.0) < 0.05
            except (KeyError, IndexError, TypeError, ValueError, ZeroDivisionError):
                valid = False
            if not valid:
                self.diagnostics.error("NATIVEVN_VIDEO_FORMAT_INVALID", "loop video must be 1920x1080, 24 fps, and 12 seconds", self.relative(path))

    def _validate_git_private_voice(self) -> None:
        try:
            top = subprocess.run(
                ["git", "-C", str(self.root), "rev-parse", "--show-toplevel"],
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                check=False,
            )
        except OSError:
            top = None
        if top is None or top.returncode != 0:
            self.missing("NATIVEVN_GIT_TREE_UNAVAILABLE", "Git tree is unavailable for private-voice tracking validation", ".")
            return
        tracked = subprocess.run(
            ["git", "-C", top.stdout.strip(), "ls-files", "--", "Examples/NativeVN/.local/voice"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        if tracked.returncode != 0:
            self.missing("NATIVEVN_GIT_TREE_UNAVAILABLE", "Git tracked-file query failed", ".")
        elif tracked.stdout.strip():
            self.diagnostics.error("NATIVEVN_PRIVATE_VOICE_TRACKED", "public Git tree contains private .local voice output", ".local/voice")


def _paeth(left: int, up: int, upper_left: int) -> int:
    estimate = left + up - upper_left
    distances = (abs(estimate - left), abs(estimate - up), abs(estimate - upper_left))
    return (left, up, upper_left)[distances.index(min(distances))]


def inspect_png(path: Path) -> PngInfo:
    try:
        data = path.read_bytes()
    except OSError as error:
        raise ToolFailure("NATIVEVN_PNG_INVALID", "PNG is unreadable") from error
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        raise ToolFailure("NATIVEVN_PNG_INVALID", "PNG signature is invalid")
    offset = 8
    width = height = bit_depth = color_type = interlace = -1
    compressed = bytearray()
    while offset + 12 <= len(data):
        length = struct.unpack_from(">I", data, offset)[0]
        chunk_type = data[offset + 4 : offset + 8]
        payload_start = offset + 8
        payload_end = payload_start + length
        if payload_end + 4 > len(data):
            raise ToolFailure("NATIVEVN_PNG_INVALID", "PNG chunk is truncated")
        expected_crc = struct.unpack_from(">I", data, payload_end)[0]
        if zlib.crc32(chunk_type + data[payload_start:payload_end]) & 0xFFFFFFFF != expected_crc:
            raise ToolFailure("NATIVEVN_PNG_INVALID", "PNG chunk checksum is invalid")
        if chunk_type == b"IHDR":
            width, height, bit_depth, color_type, _compression, _filter, interlace = struct.unpack(">IIBBBBB", data[payload_start:payload_end])
        elif chunk_type == b"IDAT":
            compressed.extend(data[payload_start:payload_end])
        elif chunk_type == b"IEND":
            break
        offset = payload_end + 4
    channels = {0: 1, 2: 3, 4: 2, 6: 4}.get(color_type)
    if width <= 0 or height <= 0 or bit_depth != 8 or channels is None or interlace != 0:
        raise ToolFailure("NATIVEVN_PNG_FORMAT_UNSUPPORTED", "PNG must be non-interlaced 8-bit grayscale, RGB, gray-alpha, or RGBA")
    try:
        import numpy as np
        from PIL import Image

        with Image.open(path) as opened:
            rgba = np.asarray(opened.convert("RGBA"), dtype=np.uint8)
        red = rgba[:, :, 0]
        green = rgba[:, :, 1]
        blue = rgba[:, :, 2]
        alpha = rgba[:, :, 3]
        transparent_mask = alpha < 250
        opaque_mask = ~transparent_mask
        chroma_mask = opaque_mask & (((green > 245) & (red < 20) & (blue < 20)) | ((red > 245) & (blue > 245) & (green < 20)))
        return PngInfo(
            width,
            height,
            color_type,
            color_type in {4, 6},
            int(np.count_nonzero(transparent_mask)),
            int(np.count_nonzero(opaque_mask)),
            int(np.count_nonzero(chroma_mask)),
        )
    except ImportError:
        pass
    try:
        raw = zlib.decompress(compressed)
    except zlib.error as error:
        raise ToolFailure("NATIVEVN_PNG_INVALID", "PNG image data is invalid") from error
    stride = width * channels
    if len(raw) != height * (stride + 1):
        raise ToolFailure("NATIVEVN_PNG_INVALID", "PNG scanline size is inconsistent")
    previous = bytearray(stride)
    position = 0
    transparent = opaque = chroma = 0
    for _row in range(height):
        filter_type = raw[position]
        position += 1
        scanline = bytearray(raw[position : position + stride])
        position += stride
        for index in range(stride):
            left = scanline[index - channels] if index >= channels else 0
            up = previous[index]
            upper_left = previous[index - channels] if index >= channels else 0
            if filter_type == 1:
                scanline[index] = (scanline[index] + left) & 0xFF
            elif filter_type == 2:
                scanline[index] = (scanline[index] + up) & 0xFF
            elif filter_type == 3:
                scanline[index] = (scanline[index] + ((left + up) // 2)) & 0xFF
            elif filter_type == 4:
                scanline[index] = (scanline[index] + _paeth(left, up, upper_left)) & 0xFF
            elif filter_type != 0:
                raise ToolFailure("NATIVEVN_PNG_INVALID", "PNG uses an invalid scanline filter")
        for index in range(0, stride, channels):
            if color_type == 0:
                red = green = blue = scanline[index]
                alpha = 255
            elif color_type == 2:
                red, green, blue = scanline[index : index + 3]
                alpha = 255
            elif color_type == 4:
                red = green = blue = scanline[index]
                alpha = scanline[index + 1]
            else:
                red, green, blue, alpha = scanline[index : index + 4]
            if alpha < 250:
                transparent += 1
            else:
                opaque += 1
                if (green > 245 and red < 20 and blue < 20) or (red > 245 and blue > 245 and green < 20):
                    chroma += 1
        previous = scanline
    return PngInfo(width, height, color_type, color_type in {4, 6}, transparent, opaque, chroma)


def _decode_pcm24(data: bytes) -> Iterable[int]:
    usable = len(data) - len(data) % 3
    for index in range(0, usable, 3):
        value = data[index] | (data[index + 1] << 8) | (data[index + 2] << 16)
        yield value - 0x1000000 if value & 0x800000 else value


def inspect_wav(path: Path) -> WavInfo:
    try:
        with wave.open(str(path), "rb") as stream:
            channels = stream.getnchannels()
            sample_width = stream.getsampwidth()
            sample_rate = stream.getframerate()
            frame_count = stream.getnframes()
            compression = stream.getcomptype()
            peak = 0
            squared = 0.0
            sample_count = 0
            clipped = 0
            while True:
                data = stream.readframes(65_536)
                if not data:
                    break
                if sample_width != 3:
                    continue
                for sample in _decode_pcm24(data):
                    absolute = abs(sample)
                    peak = max(peak, absolute)
                    squared += float(sample) * float(sample)
                    clipped += absolute >= 8_388_607
                    sample_count += 1
    except (OSError, EOFError, wave.Error) as error:
        raise ToolFailure("NATIVEVN_WAV_INVALID", "WAV file is unreadable or invalid") from error
    if compression != "NONE" or frame_count <= 0 or sample_rate <= 0:
        raise ToolFailure("NATIVEVN_WAV_INVALID", "WAV must contain uncompressed PCM frames")
    full_scale = 8_388_607.0
    rms = math.sqrt(squared / sample_count) / full_scale if sample_count else 0.0
    rms_dbfs = 20.0 * math.log10(max(rms, 1e-12))
    return WavInfo(frame_count / sample_rate, sample_rate, sample_width * 8, channels, peak / full_scale, rms_dbfs, clipped)


def _default_root() -> Path:
    return Path(__file__).resolve().parents[2] / "Examples" / "NativeVN"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=_default_root())
    parser.add_argument("--release", action="store_true", help="require the complete formal content-pack shape")
    args = parser.parse_args(argv)
    validator = ContentPackValidator(args.root, release=args.release)
    try:
        diagnostics, summary = validator.validate()
    except OSError:
        diagnostics = Diagnostics()
        diagnostics.error("NATIVEVN_VALIDATION_IO_FAILED", "content validation failed during a filesystem or process operation")
        summary = validator.summary()
    diagnostics.emit_json(summary=summary)
    return 2 if diagnostics.failed else 0


if __name__ == "__main__":
    sys.exit(main())
