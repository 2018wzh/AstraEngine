"""Lower the original Director scene DSL into typed presentation semantics."""

from __future__ import annotations

from collections import Counter
from hashlib import sha256
import json


class DirectorSceneSemanticError(ValueError):
    """Raised when a scene operation cannot be represented exactly."""


def build_scene_semantic_ir(scene_dsl: dict) -> tuple[dict, dict]:
    if scene_dsl.get("schema") != "tsuinosora.director_scene_dsl_ir.v1":
        raise DirectorSceneSemanticError("Director scene DSL schema is invalid")
    scenes = []
    counts: Counter[str] = Counter()
    source_operation_count = 0
    for scene in scene_dsl.get("scenes", []):
        source_operations = scene.get("operations")
        if not isinstance(source_operations, list):
            raise DirectorSceneSemanticError("Director scene operation list is invalid")
        source_operation_count += sum(1 for _ in _walk(source_operations))
        operations = _lower_operations(source_operations, scene.get("movie_id"), scene.get("frame"))
        counts.update(operation["kind"] for operation in _walk(operations))
        scenes.append(
            {
                "movie_id": scene["movie_id"],
                "frame": scene["frame"],
                "source_resource_id": scene["source_resource_id"],
                "source_sha256": scene["source_sha256"],
                "operations": operations,
            }
        )
    detailed = {"schema": "tsuinosora.director_scene_semantic_ir.v1", "scenes": scenes}
    report = {
        "schema": "tsuinosora.director_scene_semantic_report.v1",
        "status": "pass",
        "scene_count": len(scenes),
        "source_operation_count": source_operation_count,
        "semantic_operation_count": sum(counts.values()),
        "semantic_kind_counts": dict(sorted(counts.items())),
        "scene_semantic_sha256": _hash_json(detailed),
        "diagnostics": [],
        "redaction": {
            "paths": "alias_or_report_relative_only",
            "payload": "omitted",
            "commercial_text": "private_ir_only",
        },
    }
    return detailed, report


def _lower_operations(operations: list[dict], movie_id: object, frame: object) -> list[dict]:
    lowered: list[dict] = []
    for operation in operations:
        kind = operation.get("kind")
        termination = operation.get("termination")
        if kind == "scene_transaction":
            lowered.append(
                {
                    "kind": "transaction",
                    "operations": _lower_operations(operation.get("operations", []), movie_id, frame),
                }
            )
        elif kind in {"talk", "mono", "monoreturn"}:
            lowered.append(
                {
                    "kind": "reading",
                    "mode": kind,
                    "termination": termination,
                    "events": _lower_operations(operation.get("events", []), movie_id, frame),
                }
            )
        elif kind == "text":
            text = operation.get("text")
            if not isinstance(text, str) or not text:
                _fail(movie_id, frame, "reading text is empty")
            lowered.append({"kind": "text", "text": text})
        elif kind == "preload":
            lowered.extend(
                {"kind": "preload_member", "member": value}
                for value in _nonempty_values(operation, movie_id, frame)
            )
        elif kind == "audio":
            lowered.extend(_lower_audio(_nonempty_values(operation, movie_id, frame), movie_id, frame))
        elif kind == "back":
            for value in _nonempty_values(operation, movie_id, frame):
                lowered.append(
                    {"kind": "hide_layer", "layer": "background"}
                    if value == "-"
                    else {"kind": "show_member", "layer": "background", "member": value, "opacity": 100}
                )
        elif kind == "char":
            lowered.extend(_lower_paired_members(operation, movie_id, frame, "character", {"+": 100, "+-": 50}))
        elif kind == "event":
            lowered.extend(
                _lower_paired_members(
                    operation,
                    movie_id,
                    frame,
                    "event",
                    {"+": 100, "*": 100, "*-": 50},
                    hide_tokens={"-": "transition_out", "--": "immediate"},
                )
            )
        elif kind == "sky":
            for value in _nonempty_values(operation, movie_id, frame):
                if value not in {"+", "-"}:
                    _fail(movie_id, frame, "sky control token is unsupported")
                lowered.append({"kind": "set_layer_visibility", "layer": "sky", "visible": value == "+"})
        elif kind == "eye":
            for value in _nonempty_values(operation, movie_id, frame):
                lowered.append(
                    {"kind": "hide_layer", "layer": "eye"}
                    if value == "-"
                    else {"kind": "show_eye", "member_suffix": value}
                )
        elif kind == "shade":
            for value in _nonempty_values(operation, movie_id, frame):
                if value not in {"+-", "-"}:
                    _fail(movie_id, frame, "shade control token is unsupported")
                lowered.append({"kind": "set_shade", "opacity": 70 if value == "+-" else 0})
        elif kind in {"reset", "clear", "shake", "skipon", "skipoff", "wait", "waitse", "waitmusic"}:
            lowered.append({"kind": kind})
        else:
            _fail(movie_id, frame, f"scene operation {kind!r} has no semantic lowering")
    return lowered


def _lower_audio(values: list[str], movie_id: object, frame: object) -> list[dict]:
    result = []
    index = 0
    while index < len(values):
        token = values[index]
        if token in {"L+", "S"}:
            if index + 1 >= len(values):
                _fail(movie_id, frame, f"audio token {token} requires a member")
            result.append(
                {
                    "kind": "play_audio",
                    "bus": "bgm" if token == "L+" else "se",
                    "member": values[index + 1],
                    "looped": token == "L+",
                    "fade_frames": 240 if token == "L+" else 0,
                }
            )
            index += 2
            continue
        stop = {"L-": ("bgm", 240), "L--": ("bgm", 60), "L0": ("bgm", 0), "S0": ("se", 0)}.get(token)
        if stop is None:
            _fail(movie_id, frame, f"audio control token {token!r} is unsupported")
        result.append({"kind": "stop_audio", "bus": stop[0], "fade_frames": stop[1]})
        index += 1
    return result


def _lower_paired_members(
    operation: dict,
    movie_id: object,
    frame: object,
    layer: str,
    show_tokens: dict[str, int],
    hide_tokens: dict[str, str] | None = None,
) -> list[dict]:
    hide_tokens = hide_tokens or {"-": "immediate"}
    values = _nonempty_values(operation, movie_id, frame)
    result = []
    index = 0
    while index < len(values):
        token = values[index]
        if token in show_tokens:
            if index + 1 >= len(values):
                _fail(movie_id, frame, f"{layer} token {token} requires a member")
            result.append(
                {
                    "kind": "show_member",
                    "layer": layer,
                    "member": values[index + 1],
                    "opacity": show_tokens[token],
                    "transition": "transition_in" if token == "+" and layer == "event" else "immediate",
                }
            )
            index += 2
            continue
        if token in hide_tokens:
            result.append({"kind": "hide_layer", "layer": layer, "transition": hide_tokens[token]})
            index += 1
            continue
        if operation.get("termination") == "end_of_scene":
            result.append(
                {
                    "kind": "original_case_miss",
                    "controller": layer,
                    "token": token,
                }
            )
            index += 1
            continue
        _fail(movie_id, frame, f"{layer} control token {token!r} is unsupported")
    return result


def _nonempty_values(operation: dict, movie_id: object, frame: object) -> list[str]:
    values = operation.get("values")
    if not isinstance(values, list) or not values or any(not isinstance(value, str) or not value for value in values):
        _fail(movie_id, frame, "scene parameter values are invalid")
    return values


def _walk(operations: list[dict]):
    for operation in operations:
        yield operation
        for key in ("operations", "events"):
            children = operation.get(key)
            if isinstance(children, list):
                yield from _walk(children)


def _fail(movie_id: object, frame: object, message: str):
    raise DirectorSceneSemanticError(f"movie {movie_id} frame {frame}: {message}")


def _hash_json(value: object) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return f"sha256:{sha256(encoded).hexdigest()}"
