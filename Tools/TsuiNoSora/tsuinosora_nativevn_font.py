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

from tsuinosora_constants import *
from tsuinosora_diagnostics import _is_safe_symbol, _write_json
from tsuinosora_rendering import _read_json, _safe_identifier
from tsuinosora_script_source_map import _duplicate_choice_diagnostics, _duplicate_route_conflict_diagnostics

__all__ = ['_copy_tsuinosora_ui_template', '_copy_tsuinosora_ui_font', '_collect_tsuinosora_ui_codepoints', '_font_coverage_ranges', '_cmap_format4_supported', '_cmap_format12_supported', '_merge_codepoint_ranges', '_expand_coverage', '_render_font_coverage_yaml', '_write_asset_sidecar', '_asset_id_path', '_asset_type_for_native_path', '_cook_processor_for_asset_type', '_nativevn_route_diagnostics', '_nativevn_package_input_files', '_nativevn_file_record']


def _copy_tsuinosora_ui_template(work_root: Path, nativevn_root: Path) -> None:
    repository_root = Path(__file__).resolve().parents[2]
    template_root = repository_root / "Examples" / "TsuiNoSora" / "ProjectTemplate"
    if not template_root.is_dir():
        raise FileNotFoundError("TsuiNoSora UI project template is missing")
    for source in sorted(path for path in template_root.rglob("*") if path.is_file()):
        relative = source.relative_to(template_root)
        target = nativevn_root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    binding_path = work_root / "private" / "director_asset_bindings.json"
    if binding_path.is_file():
        binding_ir = _read_json(binding_path)
        def unique_stage_asset(layer_name: str) -> str:
            asset_ids = {
                str(layer.get("binding", {}).get("asset_id", ""))
                for layout in binding_ir.get("stage_layouts", [])
                if isinstance(layout, dict)
                for name, layer in layout.get("layers", {}).items()
                if name == layer_name and isinstance(layer, dict)
            }
            asset_id = next(iter(asset_ids), "")
            if len(asset_ids) != 1 or not _is_safe_symbol(asset_id):
                raise ValueError(
                    f"Director stage layouts do not agree on one {layer_name} asset"
                )
            return asset_id

        sky_frame_id = unique_stage_asset("sky")
        dialogue_frame_id = unique_stage_asset("dialogue_frame")
        for theme_name in ("classic.json", "modern.json"):
            theme_path = nativevn_root / "Themes" / theme_name
            theme = _read_json(theme_path)
            theme.setdefault("tokens", {})["sky.frame"] = {
                "asset": f"asset:/{sky_frame_id}"
            }
            theme.setdefault("tokens", {})["dialogue.frame"] = {
                "asset": f"asset:/{dialogue_frame_id}"
            }
            _write_json(theme_path, theme)
    localization_root = nativevn_root / "Localization"
    source_locale_path = localization_root / "ja.json"
    if not source_locale_path.is_file():
        raise FileNotFoundError("typed story conversion did not produce the Japanese localization table")
    source_locale = _read_json(source_locale_path)
    system_ja = _read_json(localization_root / "ja.system.json")
    collisions = set(source_locale.get("strings", {})) & set(system_ja.get("strings", {}))
    if collisions:
        raise ValueError("system localization keys collide with converted story keys")
    source_locale["strings"].update(system_ja["strings"])
    _write_json(source_locale_path, source_locale)
    for locale in ("zh-Hans", "en"):
        system_locale = _read_json(localization_root / f"{locale}.system.json")
        localized = {
            **source_locale,
            "locale": locale,
            "strings": dict(source_locale["strings"]),
        }
        localized["strings"].update(system_locale["strings"])
        _write_json(localization_root / f"{locale}.json", localized)
    _copy_tsuinosora_ui_font(repository_root, nativevn_root)


def _copy_tsuinosora_ui_font(repository_root: Path, nativevn_root: Path) -> None:
    source = repository_root / "Examples" / "NativeVN" / "Assets" / "Fonts" / "NotoSansJP-Variable.ttf"
    expected_hash = "sha256:c2f3b4d463500a2ddcd3849cded1fceeb9fd6d1c32e6cbecd568453ba50fc68f"
    if not source.is_file() or _sha256(source) != expected_hash:
        raise FileNotFoundError("the reviewed OFL Noto Sans JP UI font is missing or has changed")
    relative_path = "native-assets/ui/fonts/NotoSansJP-Variable.ttf"
    target = nativevn_root / relative_path
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, target)
    required_codepoints = _collect_tsuinosora_ui_codepoints(nativevn_root)
    jp_coverage = _font_coverage_ranges(source, required_codepoints)
    sidecar = f"""schema: astra.asset.v1
id: asset:/font/tsuinosora-ui
source: native-assets/ui/fonts/NotoSansJP-Variable.ttf
source_hash: sha256:c2f3b4d463500a2ddcd3849cded1fceeb9fd6d1c32e6cbecd568453ba50fc68f
type: font.ttf
license: OFL-1.1
importer: astra.import.font
font:
  family: Noto Sans JP
  face_index: 0
  subset: cjk-production-required
  coverage:
{_render_font_coverage_yaml(jp_coverage)}
cook:
  processor: astra.cook.font
  target_profiles: [classic, modern]
  params: {{}}
review: accepted
"""
    target.with_name(target.name + ".astra-asset.yaml").write_text(sidecar, encoding="utf-8")

    sc_source = repository_root / "Engine" / "Fixtures" / "PublicDomainFonts" / "NotoSansSC-Variable.ttf"
    sc_expected_hash = "sha256:a3041811a78c361b1de50f953c805e0244951c21c5bd412f7232ef0d899af0da"
    if not sc_source.is_file() or _sha256(sc_source) != sc_expected_hash:
        raise FileNotFoundError("the reviewed OFL Noto Sans SC UI fallback font is missing or has changed")
    sc_relative_path = "native-assets/ui/fonts/NotoSansSC-Variable.ttf"
    sc_target = nativevn_root / sc_relative_path
    shutil.copy2(sc_source, sc_target)
    sc_coverage = _font_coverage_ranges(sc_source, required_codepoints)
    missing = required_codepoints - _expand_coverage(jp_coverage) - _expand_coverage(sc_coverage)
    if missing:
        preview = ",".join(f"U+{codepoint:04X}" for codepoint in sorted(missing)[:8])
        raise ValueError(
            "reviewed TsuiNoSora UI fonts do not cover every required codepoint: " + preview
        )
    sc_sidecar = f"""schema: astra.asset.v1
id: asset:/font/tsuinosora-ui-sc
source: native-assets/ui/fonts/NotoSansSC-Variable.ttf
source_hash: sha256:a3041811a78c361b1de50f953c805e0244951c21c5bd412f7232ef0d899af0da
type: font.ttf
license: OFL-1.1
importer: astra.import.font
font:
  family: Noto Sans SC
  face_index: 0
  subset: cjk-production-required
  coverage:
{_render_font_coverage_yaml(sc_coverage)}
cook:
  processor: astra.cook.font
  target_profiles: [classic, modern]
  params: {{}}
review: accepted
"""
    sc_target.with_name(sc_target.name + ".astra-asset.yaml").write_text(
        sc_sidecar, encoding="utf-8"
    )


def _collect_tsuinosora_ui_codepoints(nativevn_root: Path) -> set[int]:
    codepoints = set(range(32, 127))
    roots = (
        nativevn_root / "Localization",
        nativevn_root / "UI",
        nativevn_root / "Scripts",
        nativevn_root / "Controllers",
        nativevn_root / "Themes",
    )
    files = sorted(
        path
        for root in roots
        if root.is_dir()
        for path in root.rglob("*")
        if path.is_file() and path.suffix.lower() in {".json", ".astra", ".luau"}
    )
    if not files:
        raise FileNotFoundError("TsuiNoSora UI font coverage has no textual project inputs")
    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError as error:
            raise ValueError("TsuiNoSora UI source is not strict UTF-8") from error
        codepoints.update(ord(character) for character in text if character not in "\r\n\t")
    return codepoints


def _font_coverage_ranges(font_path: Path, required: set[int]) -> list[tuple[int, int]]:
    data = font_path.read_bytes()
    if len(data) < 12:
        raise ValueError("reviewed UI font has a truncated SFNT header")
    table_count = struct.unpack_from(">H", data, 4)[0]
    cmap_offset = None
    cmap_size = None
    for index in range(table_count):
        record_offset = 12 + index * 16
        if record_offset + 16 > len(data):
            raise ValueError("reviewed UI font has a truncated SFNT table directory")
        tag, offset, size = struct.unpack_from(">4s4xII", data, record_offset)
        if offset > len(data) or size > len(data) - offset:
            raise ValueError("reviewed UI font contains an out-of-bounds SFNT table")
        if tag == b"cmap":
            cmap_offset, cmap_size = offset, size
    if cmap_offset is None or cmap_size is None or cmap_size < 4:
        raise ValueError("reviewed UI font does not contain a bounded cmap table")
    cmap = memoryview(data)[cmap_offset : cmap_offset + cmap_size]
    subtable_count = struct.unpack_from(">H", cmap, 2)[0]
    supported: set[int] = set()
    accepted_subtable = False
    for index in range(subtable_count):
        record_offset = 4 + index * 8
        if record_offset + 8 > len(cmap):
            raise ValueError("reviewed UI font has a truncated cmap directory")
        platform, encoding, offset = struct.unpack_from(">HHI", cmap, record_offset)
        if platform != 0 and not (platform == 3 and encoding in {1, 10}):
            continue
        if offset + 2 > len(cmap):
            raise ValueError("reviewed UI font contains an out-of-bounds cmap subtable")
        format_id = struct.unpack_from(">H", cmap, offset)[0]
        if format_id == 4:
            accepted_subtable = True
            supported.update(_cmap_format4_supported(cmap[offset:], required))
        elif format_id == 12:
            accepted_subtable = True
            supported.update(_cmap_format12_supported(cmap[offset:], required))
    if not accepted_subtable:
        raise ValueError("reviewed UI font has no supported Unicode cmap format")
    return _merge_codepoint_ranges(supported)


def _cmap_format4_supported(cmap: memoryview, required: set[int]) -> set[int]:
    if len(cmap) < 16:
        raise ValueError("reviewed UI font has a truncated cmap format 4 subtable")
    length = struct.unpack_from(">H", cmap, 2)[0]
    segment_count = struct.unpack_from(">H", cmap, 6)[0] // 2
    if length > len(cmap) or segment_count == 0:
        raise ValueError("reviewed UI font has an invalid cmap format 4 boundary")
    table = cmap[:length]
    end_codes_offset = 14
    start_codes_offset = end_codes_offset + segment_count * 2 + 2
    deltas_offset = start_codes_offset + segment_count * 2
    range_offsets_offset = deltas_offset + segment_count * 2
    if range_offsets_offset + segment_count * 2 > len(table):
        raise ValueError("reviewed UI font has a truncated cmap format 4 segment table")
    supported = set()
    for segment in range(segment_count):
        end = struct.unpack_from(">H", table, end_codes_offset + segment * 2)[0]
        start = struct.unpack_from(">H", table, start_codes_offset + segment * 2)[0]
        delta = struct.unpack_from(">h", table, deltas_offset + segment * 2)[0]
        range_offset_position = range_offsets_offset + segment * 2
        range_offset = struct.unpack_from(">H", table, range_offset_position)[0]
        for codepoint in required:
            if codepoint > 0xFFFF or codepoint < start or codepoint > end:
                continue
            if range_offset == 0:
                glyph = (codepoint + delta) & 0xFFFF
            else:
                glyph_position = range_offset_position + range_offset + (codepoint - start) * 2
                if glyph_position + 2 > len(table):
                    raise ValueError("reviewed UI font has an out-of-bounds cmap format 4 glyph")
                glyph = struct.unpack_from(">H", table, glyph_position)[0]
                if glyph:
                    glyph = (glyph + delta) & 0xFFFF
            if glyph:
                supported.add(codepoint)
    return supported


def _cmap_format12_supported(cmap: memoryview, required: set[int]) -> set[int]:
    if len(cmap) < 16:
        raise ValueError("reviewed UI font has a truncated cmap format 12 subtable")
    length, group_count = struct.unpack_from(">II", cmap, 4)[0], struct.unpack_from(">I", cmap, 12)[0]
    if length > len(cmap) or 16 + group_count * 12 > length:
        raise ValueError("reviewed UI font has an invalid cmap format 12 boundary")
    supported = set()
    candidates = sorted(required)
    candidate_index = 0
    for group in range(group_count):
        start, end, first_glyph = struct.unpack_from(">III", cmap, 16 + group * 12)
        while candidate_index < len(candidates) and candidates[candidate_index] < start:
            candidate_index += 1
        scan = candidate_index
        while scan < len(candidates) and candidates[scan] <= end:
            codepoint = candidates[scan]
            if first_glyph + codepoint - start:
                supported.add(codepoint)
            scan += 1
    return supported


def _merge_codepoint_ranges(codepoints: set[int]) -> list[tuple[int, int]]:
    ranges: list[tuple[int, int]] = []
    for codepoint in sorted(codepoints):
        if ranges and codepoint == ranges[-1][1] + 1:
            ranges[-1] = (ranges[-1][0], codepoint)
        else:
            ranges.append((codepoint, codepoint))
    return ranges


def _expand_coverage(ranges: list[tuple[int, int]]) -> set[int]:
    return {codepoint for start, end in ranges for codepoint in range(start, end + 1)}


def _render_font_coverage_yaml(ranges: list[tuple[int, int]]) -> str:
    if not ranges:
        raise ValueError("reviewed UI font does not cover any required codepoint")
    return "\n".join(f"    - {{ start: {start}, end: {end} }}" for start, end in ranges)


def _write_asset_sidecar(
    asset_path: Path,
    native_path: str,
    resource: dict,
    semantic_asset_id: str,
) -> None:
    asset_id = f"asset:/{semantic_asset_id}"
    asset_type = _asset_type_for_native_path(native_path, str(resource.get("classification", "")))
    processor = _cook_processor_for_asset_type(asset_type)
    source_hash = str(resource.get("converted_hash") or _sha256(asset_path))
    sidecar = [
        "schema: astra.asset.v1",
        f"id: {asset_id}",
        f"source: {native_path}",
        f"source_hash: {source_hash}",
        f"type: {asset_type}",
        "license: local-user-owned",
        "importer: astra.tsui.native_asset",
        "cook:",
        f"  processor: {processor}",
        "  target_profiles:",
        "    - classic",
        "    - modern",
        "review: accepted",
        "",
    ]
    asset_path.with_name(asset_path.name + ".astra-asset.yaml").write_text("\n".join(sidecar), encoding="utf-8")


def _asset_id_path(native_path: str) -> str:
    path = native_path.removeprefix("native-assets/")
    parts = []
    for part in path.split("/"):
        stem = "".join(ch if ch.isalnum() or ch in {"_", "-"} else "_" for ch in part)
        parts.append(stem.strip("_") or "asset")
    return "native-assets/" + "/".join(parts)


def _asset_type_for_native_path(native_path: str, classification: str) -> str:
    suffix = Path(native_path).suffix.lower()
    if suffix in IMAGE_EXTS:
        return "image.rgba"
    if suffix in AUDIO_EXTS:
        return "audio.stream"
    if suffix in MOVIE_EXTS:
        return "movie.stream"
    if suffix in FONT_EXTS:
        return "font"
    return f"binary.{_safe_identifier(classification or 'asset')}"


def _cook_processor_for_asset_type(asset_type: str) -> str:
    if asset_type.startswith("image."):
        return "astra.cook.texture2d"
    if asset_type.startswith("audio."):
        return "astra.cook.audio"
    if asset_type.startswith("movie."):
        return "astra.cook.movie"
    if asset_type == "font":
        return "astra.cook.font"
    return "astra.cook.binary"


def _nativevn_route_diagnostics(routes: list[dict]) -> list[dict]:
    diagnostics = []
    for route_index, route in enumerate(routes):
        route_id = str(route.get("route_id", "")).strip()
        terminal = str(route.get("terminal", "")).strip()
        coverage = str(route.get("coverage", "")).strip()
        if not _is_safe_symbol(route_id):
            diagnostics.append(
                {
                    "code": "TSUI_NATIVEVN_ROUTE_ID_INVALID",
                    "route_index": route_index,
                    "message": "NativeVN route_id must be a safe symbol before story/scenario generation",
                }
            )
        if terminal and not _is_safe_symbol(terminal):
            diagnostics.append(
                {
                    "code": "TSUI_NATIVEVN_ROUTE_TERMINAL_INVALID",
                    "route_id": route_id or "unknown",
                    "route_index": route_index,
                    "message": "NativeVN route terminal must be a safe symbol before story/scenario generation",
                }
            )
        if coverage != "covered":
            diagnostics.append(
                {
                    "code": "TSUI_NATIVEVN_ROUTE_COVERAGE_INVALID",
                    "route_id": route_id or "unknown",
                    "route_index": route_index,
                    "message": "NativeVN route must carry covered coverage before story/scenario generation",
                }
            )
        raw_choices = route.get("choices", [])
        if raw_choices is None:
            continue
        if not isinstance(raw_choices, list):
            diagnostics.append(
                {
                    "code": "TSUI_NATIVEVN_ROUTE_CHOICES_INVALID",
                    "route_id": route_id or "unknown",
                    "route_index": route_index,
                    "message": "NativeVN route choices must be a list of safe symbols",
                }
            )
            continue
        for choice_index, choice in enumerate(raw_choices):
            choice_id = str(choice).strip()
            if not _is_safe_symbol(choice_id):
                diagnostics.append(
                    {
                        "code": "TSUI_NATIVEVN_ROUTE_CHOICE_INVALID",
                        "route_id": route_id or "unknown",
                        "route_index": route_index,
                        "choice_index": choice_index,
                        "message": "NativeVN route choice id must be a safe symbol",
                    }
                )
    diagnostics.extend(
        _duplicate_choice_diagnostics(
            routes,
            code="TSUI_NATIVEVN_ROUTE_DUPLICATE_CHOICE",
            message="NativeVN explicit route choices must not be deduped silently before story/scenario generation",
        )
    )
    diagnostics.extend(
        _duplicate_route_conflict_diagnostics(
            routes,
            code="TSUI_NATIVEVN_ROUTE_CONFLICT",
            message="NativeVN explicit routes must not reuse a route_id with conflicting terminal or choice evidence",
        )
    )
    return diagnostics


def _nativevn_package_input_files(nativevn_root: Path, section_specs: list[dict], scenario_refs: list[str]) -> list[dict]:
    records = []
    project_path = nativevn_root / "project.yaml"
    if project_path.exists():
        records.append(_nativevn_file_record(project_path, "project", "nativevn/project.yaml"))
    source_roles = {
        "Scripts": "story",
        "UI": "ui_blueprint",
        "Themes": "ui_theme",
        "Controllers": "ui_controller",
        "Localization": "localization",
        "Automation": "physical_input_sequence",
        "Profiles": "profile_manifest",
    }
    for directory, role in source_roles.items():
        root = nativevn_root / directory
        if root.exists():
            for path in sorted(item for item in root.rglob("*") if item.is_file()):
                relative = path.relative_to(nativevn_root).as_posix()
                records.append(_nativevn_file_record(path, role, f"nativevn/{relative}"))
    for spec in section_specs:
        record = _nativevn_file_record(nativevn_root / spec["path"], "package_section", f"nativevn/{spec['path']}")
        record["section_id"] = spec["id"]
        record["section_schema"] = spec["schema"]
        records.append(record)
    for ref in scenario_refs:
        records.append(_nativevn_file_record(nativevn_root / ref, "scenario_ref", f"nativevn/{ref}"))
    asset_root = nativevn_root / "native-assets"
    if asset_root.exists():
        for path in sorted(p for p in asset_root.rglob("*") if p.is_file()):
            rel = str(path.relative_to(nativevn_root)).replace("\\", "/")
            role = "asset_sidecar" if path.name.endswith(".astra-asset.yaml") else "asset"
            records.append(_nativevn_file_record(path, role, f"nativevn/{rel}"))
    return records


def _nativevn_file_record(path: Path, role: str, report_path: str) -> dict:
    return {
        "role": role,
        "path": report_path,
        "sha256": _sha256(path),
        "byte_size": path.stat().st_size,
    }
