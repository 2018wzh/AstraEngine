"""Validated private TsuiNoSora story IR to AstraVN source conversion.

The input may contain commercial text and therefore belongs under the ignored
local work root.  Returned reports contain identities, hashes and counts only.
No command is ignored: unsupported or malformed records block all story output.
"""

from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path


SCHEMA = "tsuinosora.native_story_ir.v1"
REPORT_SCHEMA = "tsuinosora.full_conversion_coverage_report.v1"
SAFE_SYMBOL = re.compile(r"^[A-Za-z0-9_.-]+$")
SAFE_ASSET_ID = re.compile(r"^[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
SUPPORTED_COMMANDS = {
    "preload",
    "stage",
    "layer",
    "background",
    "show",
    "move",
    "hide",
    "clear_layer",
    "layer_visibility",
    "shade",
    "skip_allowed",
    "bgm",
    "se",
    "voice",
    "audio_control",
    "movie",
    "transition",
    "shake",
    "text",
    "choice",
    "wait",
    "input_wait",
    "system_page",
    "mutate",
    "jump",
    "branch",
    "call",
    "return",
}
MEDIA_COMMANDS = {"preload", "background", "show", "bgm", "se", "voice", "movie"}
WAIT_COMMANDS = {"text", "choice", "wait", "input_wait", "movie"}
MAX_STATES_PER_SOURCE = 64
PHYSICAL_INPUT_TYPES = {
    "resume",
    "focus",
    "keyboard",
    "ime_preedit",
    "ime_commit",
    "pointer_move",
    "pointer_button",
    "wheel",
    "touch",
    "gamepad_connection",
    "gamepad_input",
    "advance_ticks",
    "await",
    "checkpoint",
    "shutdown",
}


def convert_native_story_ir(ir_path: Path, output_root: Path) -> dict:
    diagnostics: list[dict] = []
    try:
        payload = json.loads(ir_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        return _blocked_report("TSUI_NATIVE_STORY_IR_UNREADABLE", "private story IR is missing or invalid JSON")
    if not isinstance(payload, dict) or payload.get("schema") != SCHEMA:
        return _blocked_report("TSUI_NATIVE_STORY_IR_SCHEMA", "private story IR schema is missing or unsupported")

    sources = _validate_sources(payload.get("sources"), diagnostics)
    handlers = _validate_handlers(payload.get("handlers"), sources, diagnostics)
    stories, command_records, state_ids = _validate_stories(payload.get("stories"), handlers, diagnostics)
    _validate_command_links(command_records, state_ids, diagnostics)
    routes = _validate_routes(payload.get("routes"), command_records, state_ids, diagnostics)
    _validate_coverage(payload.get("coverage"), payload.get("source_locale"), sources, handlers, command_records, routes, diagnostics)
    if diagnostics:
        return _coverage_report(sources, handlers, command_records, routes, diagnostics)

    scripts_root = output_root / "Scripts"
    localization_root = output_root / "Localization"
    scripts_root.mkdir(parents=True, exist_ok=True)
    localization_root.mkdir(parents=True, exist_ok=True)
    strings: dict[str, str] = {}
    generated_files: list[dict] = []
    for story in stories:
        states = story["states"]
        for part, offset in enumerate(range(0, len(states), MAX_STATES_PER_SOURCE)):
            path = scripts_root / f"{story['story_id']}.{part:03}.astra"
            rendered = _render_story_part(
                story["story_id"],
                states[offset : offset + MAX_STATES_PER_SOURCE],
                strings,
                include_story=part == 0,
            )
            path.write_text(rendered, encoding="utf-8")
            generated_files.append(_file_record(path, output_root))
    locale = payload.get("source_locale")
    localization_path = localization_root / f"{locale}.json"
    localization_path.write_text(
        json.dumps(
            {"schema": "astra.vn.localization_table.v1", "locale": locale, "strings": strings},
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    generated_files.append(_file_record(localization_path, output_root))
    automation_root = output_root / "Automation"
    automation_root.mkdir(parents=True, exist_ok=True)
    for route in routes:
        automation_path = automation_root / f"{route['route_id']}.jsonl"
        _write_physical_input_sequence(automation_path, route)
        generated_files.append(_file_record(automation_path, output_root))
    report = _coverage_report(sources, handlers, command_records, routes, [])
    report["generated_files"] = generated_files
    report["localization_key_count"] = len(strings)
    report["source_locale"] = locale
    return report


def _validate_sources(value, diagnostics: list[dict]) -> dict[str, dict]:
    result: dict[str, dict] = {}
    if not isinstance(value, list) or not value:
        diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_SOURCES_MISSING", "story IR requires source records"))
        return result
    for index, source in enumerate(value):
        if not isinstance(source, dict):
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_SOURCE_INVALID", "source record must be an object", index=index))
            continue
        source_id = source.get("source_id")
        relative_path = source.get("relative_path")
        source_hash = source.get("sha256")
        kind = source.get("kind")
        if (
            not _safe(source_id)
            or not _relative(relative_path)
            or not isinstance(source_hash, str)
            or not SHA256.fullmatch(source_hash)
            or not _safe(kind)
        ):
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_SOURCE_INVALID", "source identity, relative path, hash and kind are required", index=index))
            continue
        if source_id in result:
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_SOURCE_DUPLICATE", "source id must be unique", source_id=source_id))
            continue
        result[source_id] = source
    return result


def _validate_handlers(value, sources: dict[str, dict], diagnostics: list[dict]) -> dict[str, dict]:
    result: dict[str, dict] = {}
    if not isinstance(value, list) or not value:
        diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_HANDLERS_MISSING", "story IR requires handler records"))
        return result
    for index, handler in enumerate(value):
        if not isinstance(handler, dict):
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_HANDLER_INVALID", "handler record must be an object", index=index))
            continue
        handler_id = handler.get("handler_id")
        source_id = handler.get("source_id")
        if not _safe(handler_id) or source_id not in sources or handler.get("status") != "converted":
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_HANDLER_INVALID", "handler must have a source and converted status", index=index))
            continue
        if handler_id in result:
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_HANDLER_DUPLICATE", "handler id must be unique", handler_id=handler_id))
            continue
        result[handler_id] = handler
    return result


def _validate_stories(value, handlers: dict[str, dict], diagnostics: list[dict]) -> tuple[list[dict], dict[str, dict], set[str]]:
    command_records: dict[str, dict] = {}
    state_ids: set[str] = set()
    if not isinstance(value, list) or not value:
        diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_STORIES_MISSING", "story IR requires at least one story"))
        return [], command_records, state_ids
    story_ids: set[str] = set()
    for story_index, story in enumerate(value):
        if not isinstance(story, dict) or not _safe(story.get("story_id")):
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_INVALID", "story id is missing or unsafe", index=story_index))
            continue
        if story["story_id"] in story_ids:
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_DUPLICATE", "story id must be unique", story_id=story["story_id"]))
        story_ids.add(story["story_id"])
        states = story.get("states")
        if not isinstance(states, list) or not states:
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_STATES_MISSING", "story requires states", story_id=story["story_id"]))
            continue
        for state in states:
            _validate_state(state, handlers, state_ids, command_records, diagnostics)
    return value, command_records, state_ids


def _validate_state(state, handlers, state_ids, commands, diagnostics) -> None:
    if not isinstance(state, dict) or not _safe(state.get("state_id")):
        diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_STATE_INVALID", "state id is missing or unsafe"))
        return
    state_id = state["state_id"]
    if state_id in state_ids:
        diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_STATE_DUPLICATE", "state id must be globally unique", state_id=state_id))
    state_ids.add(state_id)
    scenes = state.get("scenes")
    if not isinstance(scenes, list) or not scenes:
        diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_SCENES_MISSING", "state requires scenes", state_id=state_id))
        return
    scene_ids: set[str] = set()
    for scene in scenes:
        if not isinstance(scene, dict) or not _safe(scene.get("scene_id")) or scene["scene_id"] in scene_ids:
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_SCENE_INVALID", "scene id is missing, unsafe or duplicated", state_id=state_id))
            continue
        scene_ids.add(scene["scene_id"])
        records = scene.get("commands")
        if not isinstance(records, list) or not records:
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_COMMANDS_MISSING", "scene requires commands", scene_id=scene["scene_id"]))
            continue
        for command in records:
            _validate_command(command, handlers, commands, diagnostics)


def _validate_command(command, handlers, commands, diagnostics) -> None:
    if not isinstance(command, dict):
        diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_COMMAND_INVALID", "command must be an object"))
        return
    command_id = command.get("command_id")
    kind = command.get("kind")
    handler_id = command.get("handler_id")
    if not _safe(command_id) or kind not in SUPPORTED_COMMANDS or handler_id not in handlers:
        diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_COMMAND_INVALID", "command identity, kind or handler binding is invalid"))
        return
    if command_id in commands:
        diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_COMMAND_DUPLICATE", "command id must be unique", command_id=command_id))
        return
    if not _command_payload_valid(command):
        diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_COMMAND_PAYLOAD_INVALID", "command payload is missing required typed fields", command_id=command_id, kind=kind))
        return
    commands[command_id] = command


def _command_payload_valid(command: dict) -> bool:
    kind = command["kind"]
    if kind == "text":
        return isinstance(command.get("text"), str) and bool(command["text"]) and _optional_safe(command.get("speaker_id"))
    if kind == "choice":
        options = command.get("options")
        return isinstance(command.get("prompt"), str) and isinstance(options, list) and bool(options) and all(
            isinstance(option, dict)
            and _safe(option.get("option_id"))
            and isinstance(option.get("text"), str)
            and bool(option["text"])
            and _safe(option.get("target"))
            and _condition_valid(option.get("enabled_when"))
            for option in options
        ) and len({option["option_id"] for option in options}) == len(options)
    if kind in {"jump", "call"}:
        return _safe(command.get("target"))
    if kind == "branch":
        return (
            _safe(command.get("path"))
            and command.get("op") in {"eq", "not_eq", "less", "less_eq", "greater", "greater_eq"}
            and isinstance(command.get("value"), int)
            and _safe(command.get("then_target"))
            and _safe(command.get("else_target"))
        )
    if kind == "return":
        return True
    if kind == "mutate":
        return _safe(command.get("path")) and command.get("op") in {"set", "add", "sub"} and isinstance(command.get("value"), int)
    if kind == "wait":
        return _safe(command.get("fence"))
    if kind == "input_wait":
        return True
    if kind == "system_page":
        return command.get("page") in {"title", "quick_panel", "save", "load", "config", "backlog"}
    if kind == "preload":
        return _safe_asset(command.get("asset_id"))
    if kind == "stage":
        return (
            isinstance(command.get("width"), int)
            and command["width"] > 0
            and isinstance(command.get("height"), int)
            and command["height"] > 0
            and isinstance(command.get("safe_width"), int)
            and command["safe_width"] > 0
            and isinstance(command.get("safe_height"), int)
            and command["safe_height"] > 0
        )
    if kind == "layer":
        return (
            _safe(command.get("layer"))
            and command.get("layer_kind") in {"background", "sprite", "video", "text", "cg", "ui", "effect"}
            and isinstance(command.get("z"), int)
            and command.get("blend", "normal") in {"normal", "add", "multiply", "screen"}
            and command.get("clip", "stage") in {"stage", "safe_area"}
        )
    if kind == "clear_layer":
        return _safe(command.get("layer")) and isinstance(command.get("duration_ms", 0), int) and command.get("duration_ms", 0) >= 0
    if kind == "layer_visibility":
        return _safe(command.get("layer")) and isinstance(command.get("visible"), bool)
    if kind == "shade":
        return isinstance(command.get("opacity"), int) and 0 <= command["opacity"] <= 100
    if kind == "skip_allowed":
        return isinstance(command.get("allowed"), bool)
    if kind == "audio_control":
        if command.get("action") == "fade_stop":
            return (
                _safe(command.get("target"))
                and isinstance(command.get("duration_ms"), int)
                and command["duration_ms"] > 0
                and _safe(command.get("fence"))
            )
        return command.get("action") in {"pause", "resume", "stop"} and _safe(command.get("target"))
    if kind == "shake":
        return (
            _safe(command.get("target"))
            and isinstance(command.get("strength"), int)
            and command["strength"] >= 0
            and isinstance(command.get("duration_ms"), int)
            and command["duration_ms"] > 0
        )
    if kind == "transition":
        return _safe(command.get("preset")) and isinstance(command.get("duration_ms"), int) and command["duration_ms"] >= 0
    if kind in {"background", "bgm", "se", "voice", "movie"}:
        return _safe_asset(command.get("asset_id"))
    if kind == "show":
        return (
            _safe_asset(command.get("asset_id"))
            and _safe(command.get("character_id"))
            and _safe(command.get("layer"))
            and command.get("fit", "contain_height") in {"contain_height", "native"}
            and isinstance(command.get("opacity", 100), int)
            and 0 <= command.get("opacity", 100) <= 100
        )
    if kind == "move":
        return (
            _safe(command.get("character_id"))
            and isinstance(command.get("x"), int)
            and isinstance(command.get("y"), int)
            and isinstance(command.get("duration_ms"), int)
            and command["duration_ms"] >= 0
        )
    if kind == "hide":
        return _safe(command.get("character_id"))
    return False


def _validate_command_links(commands: dict[str, dict], state_ids: set[str], diagnostics: list[dict]) -> None:
    for command in commands.values():
        if command["kind"] in {"jump", "call"} and command["target"] not in state_ids:
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_TARGET_MISSING", "command target state does not exist", command_id=command["command_id"]))
        if command["kind"] == "choice":
            for option in command["options"]:
                if option["target"] not in state_ids:
                    diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_TARGET_MISSING", "choice target state does not exist", command_id=command["command_id"], option_id=option["option_id"]))
        if command["kind"] == "branch":
            for field in ("then_target", "else_target"):
                if command[field] not in state_ids:
                    diagnostics.append(
                        _diagnostic(
                            "TSUI_NATIVE_STORY_TARGET_MISSING",
                            "branch target state does not exist",
                            command_id=command["command_id"],
                            target_field=field,
                        )
                    )
        voice_id = command.get("voice_command_id")
        if voice_id is not None and (voice_id not in commands or commands[voice_id]["kind"] != "voice"):
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_VOICE_LINK_INVALID", "text voice command must resolve to a voice command", command_id=command["command_id"]))


def _validate_routes(value, commands: dict[str, dict], state_ids: set[str], diagnostics: list[dict]) -> list[dict]:
    if not isinstance(value, list) or not value:
        diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_ROUTES_MISSING", "story IR requires route coverage"))
        return []
    signatures: dict[str, tuple[str, tuple[str, ...]]] = {}
    known_choices = {
        option["option_id"]
        for command in commands.values()
        if command["kind"] == "choice"
        for option in command["options"]
    }
    for route in value:
        if not isinstance(route, dict) or not _safe(route.get("route_id")) or not _safe(route.get("terminal_id")):
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_ROUTE_INVALID", "route identity is missing or unsafe"))
            continue
        if route["terminal_id"] not in state_ids:
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_ROUTE_TERMINAL_MISSING", "route terminal state does not exist", route_id=route["route_id"]))
        expected_route_node_id = f"state.{route['terminal_id']}"
        if route.get("terminal_route_node_id") != expected_route_node_id:
            diagnostics.append(
                _diagnostic(
                    "TSUI_NATIVE_STORY_ROUTE_NODE_INVALID",
                    "route terminal evidence must name the compiled route graph node",
                    route_id=route["route_id"],
                )
            )
        choices = route.get("choice_ids")
        choice_sequence = route.get("choice_sequence")
        evidence = route.get("command_ids")
        if not isinstance(choices, list) or len(set(choices)) != len(choices) or not all(_safe(item) for item in choices):
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_ROUTE_CHOICES_INVALID", "route choices must be unique safe ids", route_id=route["route_id"]))
            continue
        if any(choice not in known_choices for choice in choices):
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_ROUTE_CHOICE_MISSING", "route choice evidence must resolve to a converted option", route_id=route["route_id"]))
            continue
        if (
            not isinstance(choice_sequence, list)
            or not all(_safe(item) for item in choice_sequence)
            or any(choice not in known_choices for choice in choice_sequence)
            or list(dict.fromkeys(choice_sequence)) != choices
        ):
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_ROUTE_CHOICE_SEQUENCE_INVALID", "route choice sequence must preserve every selected option occurrence", route_id=route["route_id"]))
            continue
        if not isinstance(evidence, list) or not evidence or any(item not in commands for item in evidence):
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_ROUTE_COMMANDS_INVALID", "route command evidence must resolve", route_id=route["route_id"]))
            continue
        if not _validate_input_events(route.get("input_events")):
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_ROUTE_INPUT_INVALID", "route automation must contain only serialized physical input", route_id=route["route_id"]))
            continue
        signature = (route["terminal_route_node_id"], tuple(choice_sequence))
        if route["route_id"] in signatures and signatures[route["route_id"]] != signature:
            diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_ROUTE_CONFLICT", "route id has conflicting signatures", route_id=route["route_id"]))
        signatures[route["route_id"]] = signature
    return value


def _validate_coverage(value, source_locale, sources, handlers, commands, routes, diagnostics) -> None:
    required = {"source_count": len(sources), "handler_count": len(handlers), "command_count": len(commands), "route_count": len(routes)}
    if not isinstance(value, dict) or value.get("status") != "complete" or any(value.get(key) != count for key, count in required.items()):
        diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_COVERAGE_INCOMPLETE", "declared coverage must exactly match validated records"))
    if source_locale != "ja":
        diagnostics.append(_diagnostic("TSUI_NATIVE_STORY_LOCALE_INVALID", "original TsuiNoSora story locale must be ja"))


def _render_story_part(
    story_id: str,
    states: list[dict],
    strings: dict[str, str],
    *,
    include_story: bool,
) -> str:
    lines = [f"story {story_id} #@id story.{story_id}"] if include_story else []
    for state in states:
        lines.extend(["", f"state {state['state_id']} #@id state.{state['state_id']}"])
        for scene in state["scenes"]:
            lines.append(f"  scene {scene['scene_id']} #@id scene.{scene['scene_id']}")
            for command in scene["commands"]:
                lines.extend(_render_command(command, strings))
    return "\n".join(lines) + "\n"


def _render_command(command: dict, strings: dict[str, str]) -> list[str]:
    kind = command["kind"]
    command_id = command["command_id"]
    prefix = "    "
    stable = f"#@id {command_id}"
    if kind == "text":
        key = f"story.{command_id}"
        strings[key] = command["text"]
        speaker = f" speaker:{command['speaker_id']}" if command.get("speaker_id") else ""
        voice = f" voice:{command['voice_command_id']}" if command.get("voice_command_id") else ""
        return [f"{prefix}text key:{key}{speaker}{voice} {stable}"]
    if kind == "choice":
        key = f"story.{command_id}.prompt"
        strings[key] = command["prompt"]
        lines = [f"{prefix}choice key:{key} {stable}"]
        for option in command["options"]:
            option_key = f"story.{command_id}.option.{option['option_id']}"
            strings[option_key] = option["text"]
            enabled_when = ""
            if option.get("enabled_when") is not None:
                condition = option["enabled_when"]
                enabled_when = (
                    f" when:{condition['path']},{condition['op']},{condition['value']}"
                )
            lines.append(
                f"      option key:{option_key} target:{option['target']}{enabled_when} "
                f"#@id {option['option_id']}"
            )
        return lines
    if kind in {"jump", "call"}:
        return [f"{prefix}{kind} target:{command['target']} {stable}"]
    if kind == "branch":
        return [
            f"{prefix}branch path:{command['path']} op:{command['op']} value:{command['value']} "
            f"then:{command['then_target']} else:{command['else_target']} {stable}"
        ]
    if kind == "return":
        return [f"{prefix}return {stable}"]
    if kind == "mutate":
        op = {"set": "=", "add": "+=", "sub": "-="}[command["op"]]
        return [f"{prefix}mutate {command['path']} {op} {command['value']} {stable}"]
    if kind == "wait":
        return [f"{prefix}wait fence:{command['fence']} {stable}"]
    if kind == "input_wait":
        return [f"{prefix}input_wait {stable}"]
    if kind == "system_page":
        return [f"{prefix}system_page kind:{command['page']} {stable}"]
    if kind == "transition":
        return [f"{prefix}transition preset:{command['preset']} duration:{command['duration_ms']} {stable}"]
    if kind == "show":
        pose = f" pose:{command['pose']}" if _safe(command.get("pose")) else ""
        at = f" at:{command['at']}" if _safe(command.get("at")) else ""
        fit = f" fit:{command['fit']}" if command.get("fit") else ""
        opacity = command.get("opacity", 100) / 100
        return [f"{prefix}show id:{command['character_id']} asset:asset:/{command['asset_id']}{pose} layer:{command['layer']}{at}{fit} opacity:{opacity:g} {stable}"]
    if kind == "move":
        return [
            f"{prefix}move id:{command['character_id']} x:{command['x']} y:{command['y']} "
            f"duration:{command['duration_ms']} {stable}"
        ]
    if kind == "hide":
        return [f"{prefix}hide id:{command['character_id']} {stable}"]
    if kind == "background":
        return [f"{prefix}background asset:asset:/{command['asset_id']} layer:{command.get('layer', 'bg')} {stable}"]
    if kind in {"bgm", "se", "voice"}:
        loop = f" loop:{str(command['loop']).lower()}" if isinstance(command.get("loop"), bool) else ""
        fade = f" fade:{command['fade_ms']}" if isinstance(command.get("fade_ms"), int) else ""
        sync = ""
        if _safe(command.get("fence")):
            sync = f" sync:fence fence:{command['fence']}"
        return [f"{prefix}{kind} id:{command.get('audio_id', command_id)} asset:asset:/{command['asset_id']}{loop}{fade}{sync} {stable}"]
    if kind == "movie":
        loop = str(bool(command.get("loop", False))).lower()
        end = command.get("end", "wait")
        return [f"{prefix}movie layer:{command.get('layer', 'video')} asset:asset:/{command['asset_id']} loop:{loop} end:{end} {stable}"]
    if kind == "preload":
        return [f"{prefix}preload asset:asset:/{command['asset_id']} {stable}"]
    if kind == "stage":
        return [
            f"{prefix}stage viewport:{command['width']}x{command['height']} "
            f"safe_area:{command['safe_width']}:{command['safe_height']} {stable}"
        ]
    if kind == "layer":
        return [
            f"{prefix}layer id:{command['layer']} kind:{command['layer_kind']} z:{command['z']} "
            f"blend:{command.get('blend', 'normal')} clip:{command.get('clip', 'stage')} {stable}"
        ]
    if kind == "clear_layer":
        return [f"{prefix}clear_layer layer:{command['layer']} duration:{command.get('duration_ms', 0)} {stable}"]
    if kind == "layer_visibility":
        return [f"{prefix}layer_visibility layer:{command['layer']} visible:{str(command['visible']).lower()} {stable}"]
    if kind == "shade":
        return [f"{prefix}shade opacity:{command['opacity'] / 100:g} {stable}"]
    if kind == "skip_allowed":
        return [f"{prefix}skip_allowed allowed:{str(command['allowed']).lower()} {stable}"]
    if kind == "audio_control":
        timing = ""
        if command["action"] == "fade_stop":
            timing = f" duration:{command['duration_ms']} fence:{command['fence']}"
        return [
            f"{prefix}audio action:{command['action']} target:{command['target']}{timing} {stable}"
        ]
    if kind == "shake":
        return [
            f"{prefix}shake target:{command['target']} strength:{command['strength'] / 100:g} "
            f"duration:{command['duration_ms']} {stable}"
        ]
    raise AssertionError(f"validated command kind not rendered: {kind}")


def _validate_input_events(value) -> bool:
    if not isinstance(value, list) or not value:
        return False
    last_tick = -1
    for item in value:
        if not isinstance(item, dict) or set(item) != {"tick", "event"}:
            return False
        tick = item.get("tick")
        event = item.get("event")
        if not isinstance(tick, int) or tick < last_tick or not isinstance(event, dict):
            return False
        if event.get("type") not in PHYSICAL_INPUT_TYPES:
            return False
        if any(key in event for key in {"route_id", "option_id", "command_id", "player_command"}):
            return False
        last_tick = tick
    return value[-1]["event"].get("type") == "shutdown"


def _write_physical_input_sequence(path: Path, route: dict) -> None:
    session = f"tsui.{route['route_id']}"
    lines = []
    for sequence, item in enumerate(route["input_events"], start=1):
        lines.append(
            json.dumps(
                {
                    "schema": "astra.user_input_sequence.v1",
                    "session": session,
                    "sequence": sequence,
                    "tick": item["tick"],
                    "event": item["event"],
                },
                ensure_ascii=False,
                separators=(",", ":"),
            )
        )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _coverage_report(sources, handlers, commands, routes, diagnostics) -> dict:
    kind_counts: dict[str, int] = {}
    for command in commands.values():
        kind = command["kind"]
        kind_counts[kind] = kind_counts.get(kind, 0) + 1
    return {
        "schema": REPORT_SCHEMA,
        "status": "blocked" if diagnostics else "pass",
        "counts": {
            "source_resources": len(sources),
            "script_handlers": len(handlers),
            "commands": len(commands),
            "routes": len(routes),
            "terminals": len({route.get("terminal_id") for route in routes if isinstance(route, dict)}),
            "choices": sum(len(route.get("choice_ids", [])) for route in routes if isinstance(route, dict)),
            "media_commands": sum(1 for command in commands.values() if command["kind"] in MEDIA_COMMANDS),
            "wait_commands": sum(1 for command in commands.values() if command["kind"] in WAIT_COMMANDS),
        },
        "command_kind_counts": dict(sorted(kind_counts.items())),
        "diagnostics": diagnostics,
        "redaction": {"paths": "relative_only", "payload": "omitted", "commercial_text": "omitted"},
    }


def _blocked_report(code: str, message: str) -> dict:
    return _coverage_report({}, {}, {}, [], [_diagnostic(code, message)])


def _diagnostic(code: str, message: str, **fields) -> dict:
    return {"code": code, **fields, "message": message}


def _safe(value) -> bool:
    return isinstance(value, str) and bool(SAFE_SYMBOL.fullmatch(value))


def _safe_asset(value) -> bool:
    return isinstance(value, str) and bool(SAFE_ASSET_ID.fullmatch(value))


def _optional_safe(value) -> bool:
    return value is None or _safe(value)


def _condition_valid(value) -> bool:
    return value is None or (
        isinstance(value, dict)
        and _safe(value.get("path"))
        and value.get("op") in {"eq", "not_eq", "less", "less_eq", "greater", "greater_eq"}
        and isinstance(value.get("value"), int)
    )


def _relative(value) -> bool:
    if not isinstance(value, str) or not value or "\\" in value or value.startswith("/"):
        return False
    return all(part not in {"", ".", ".."} for part in value.split("/"))


def _file_record(path: Path, root: Path) -> dict:
    data = path.read_bytes()
    return {
        "relative_path": path.relative_to(root).as_posix(),
        "sha256": hashlib.sha256(data).hexdigest(),
        "byte_size": len(data),
    }
