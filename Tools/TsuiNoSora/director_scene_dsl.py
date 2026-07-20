"""Strict parser for the private TsuiNoSora Director scene text DSL."""

from __future__ import annotations

from collections import Counter
from hashlib import sha256
import json


class DirectorSceneDslError(ValueError):
    """Raised when a scene text cannot be represented without guessing."""


PARAMETER_BLOCKS = {"preload", "audio", "sky", "eye", "back", "char", "event", "shade"}
SELF_CONTROLS = {"reset", "clear", "shake", "skipon", "skipoff", "wait", "waitse", "waitmusic"}


def build_scene_dsl_ir(story_source: dict) -> tuple[dict, dict]:
    """Parse every bound STXT scene while keeping commercial strings private."""

    if story_source.get("schema") != "tsuinosora.director_story_source.v1":
        raise DirectorSceneDslError("Director story source schema is invalid")
    scenes: list[dict] = []
    operation_counts: Counter[str] = Counter()
    termination_counts: Counter[str] = Counter()
    source_line_count = 0
    for movie in story_source.get("movies", []):
        movie_id = movie.get("movie_id")
        if not isinstance(movie_id, str):
            raise DirectorSceneDslError("Director story movie identity is invalid")
        for label in movie.get("labels", []):
            scene_text = label.get("scene_text")
            if scene_text is None:
                continue
            text = scene_text.get("text")
            if not isinstance(text, str):
                raise DirectorSceneDslError("Director scene text is invalid")
            lines = [line.strip() for line in text.splitlines() if line.strip()]
            parser = _SceneParser(movie_id, label.get("frame"), lines)
            operations = parser.parse()
            operation_counts.update(_walk_operation_kinds(operations))
            termination_counts.update(_walk_terminations(operations))
            source_line_count += len(lines)
            scenes.append(
                {
                    "movie_id": movie_id,
                    "frame": label.get("frame"),
                    "label": label.get("label"),
                    "label_sha256": label.get("label_sha256"),
                    "source_resource_id": scene_text.get("resource_id"),
                    "source_sha256": scene_text.get("source_sha256"),
                    "source_line_count": len(lines),
                    "operations": operations,
                }
            )
    expected = sum(
        movie.get("coverage", {}).get("scene_text_binding_count", 0)
        for movie in story_source.get("movies", [])
    )
    if len(scenes) != expected:
        raise DirectorSceneDslError("scene DSL coverage does not match story source bindings")
    detailed = {"schema": "tsuinosora.director_scene_dsl_ir.v1", "scenes": scenes}
    report = {
        "schema": "tsuinosora.director_scene_dsl_report.v1",
        "status": "pass",
        "source_scene_count": expected,
        "converted_scene_count": len(scenes),
        "source_line_count": source_line_count,
        "operation_counts": dict(sorted(operation_counts.items())),
        "termination_counts": dict(sorted(termination_counts.items())),
        "scene_dsl_sha256": _hash_json(detailed),
        "diagnostics": [],
        "redaction": {
            "paths": "alias_or_report_relative_only",
            "payload": "omitted",
            "commercial_text": "private_ir_only",
        },
    }
    return detailed, report


class _SceneParser:
    def __init__(self, movie_id: str, frame: object, lines: list[str]) -> None:
        self.movie_id = movie_id
        self.frame = frame
        self.lines = lines
        self.index = 0

    def parse(self) -> list[dict]:
        operations = self._parse_sequence(None, "top")
        if self.index != len(self.lines):
            self._fail("scene parser did not consume every source line")
        return operations

    def _parse_sequence(self, terminator: str | None, context: str) -> list[dict]:
        operations: list[dict] = []
        while self.index < len(self.lines):
            line = self.lines[self.index]
            if terminator is not None and line == terminator:
                self.index += 1
                return operations
            if line == "{":
                self.index += 1
                operations.append(
                    {
                        "kind": "scene_transaction",
                        "operations": self._parse_sequence("}", "scene_transaction"),
                    }
                )
                continue
            tag = _tag(line)
            if tag is None:
                self._fail(f"orphan text in {context}")
            name, closing, self_closing = tag
            if closing:
                self._fail(f"unexpected closing tag {name}")
            if self_closing:
                if name not in SELF_CONTROLS:
                    self._fail(f"unsupported self-closing tag {name}")
                self.index += 1
                operations.append({"kind": name})
                continue
            if name in PARAMETER_BLOCKS:
                operations.append(self._parse_parameter_block(name))
                continue
            if name == "talk":
                operations.append(self._parse_dialogue_block("talk", "</talk>"))
                continue
            if name == "mono":
                operations.append(self._parse_dialogue_block("mono", ("</mono>", "</monoescape>")))
                continue
            if name == "monoreturn":
                self.index += 1
                operations.append(self._parse_mono_continuation())
                continue
            self._fail(f"unsupported opening tag {name}")
        if terminator is not None:
            self._fail(f"missing terminator {terminator}")
        return operations

    def _parse_parameter_block(self, name: str) -> dict:
        self.index += 1
        values: list[str] = []
        closing = f"</{name}>"
        while self.index < len(self.lines) and self.lines[self.index] != closing:
            values.append(self.lines[self.index])
            self.index += 1
        if not values:
            self._fail(f"empty {name} parameter block")
        if self.index >= len(self.lines):
            return {"kind": name, "termination": "end_of_scene", "values": values}
        self.index += 1
        return {"kind": name, "termination": "close", "values": values}

    def _parse_dialogue_block(self, kind: str, closing: str | tuple[str, ...]) -> dict:
        self.index += 1
        events, termination = self._parse_reading_events(kind, closing)
        if not any(event["kind"] == "text" for event in events):
            self._fail(f"empty {kind} block")
        return {"kind": kind, "termination": termination, "events": events}

    def _parse_mono_continuation(self) -> dict:
        events, termination = self._parse_reading_events("mono", ("</mono>", "</monoescape>"))
        if not any(event["kind"] == "text" for event in events):
            self._fail("empty monoreturn continuation")
        return {"kind": "monoreturn", "termination": termination, "events": events}

    def _parse_reading_events(
        self, kind: str, closing: str | tuple[str, ...]
    ) -> tuple[list[dict], str]:
        terminators = (closing,) if isinstance(closing, str) else closing
        events: list[dict] = []
        while self.index < len(self.lines):
            line = self.lines[self.index]
            if line in terminators:
                self.index += 1
                return events, "escape" if line == "</monoescape>" else "close"
            if line == "{" and kind == "mono":
                self.index += 1
                events.append(
                    {
                        "kind": "scene_transaction",
                        "operations": self._parse_sequence("}", "mono_scene_transaction"),
                    }
                )
                continue
            tag = _tag(line)
            if tag is None:
                if line == "}":
                    self._fail(f"unexpected scene transaction close inside {kind}")
                events.append({"kind": "text", "text": line})
                self.index += 1
                continue
            name, tag_closing, self_closing = tag
            if tag_closing:
                self._fail(f"unexpected closing tag {name} inside {kind}")
            if self_closing:
                allowed = {"waitse", "shake"} if kind == "talk" else {"clear", "skipon", "skipoff"}
                if name not in allowed:
                    self._fail(f"unsupported {name} control inside {kind}")
                self.index += 1
                events.append({"kind": name})
                continue
            allowed_blocks = {"audio"} if kind == "talk" else {"audio", "back", "char", "shade"}
            if name not in allowed_blocks:
                self._fail(f"unsupported {name} block inside {kind}")
            events.append(self._parse_parameter_block(name))
        self._fail(f"missing {kind} terminator")

    def _fail(self, message: str) -> None:
        raise DirectorSceneDslError(
            f"scene DSL failed for movie {self.movie_id} frame {self.frame} line {self.index + 1}: {message}"
        )


def _tag(line: str) -> tuple[str, bool, bool] | None:
    if not (line.startswith("<") and line.endswith(">")):
        return None
    body = line[1:-1]
    closing = body.startswith("/")
    self_closing = body.endswith("/")
    name = body.strip("/").strip().lower()
    if not name or any(character not in "abcdefghijklmnopqrstuvwxyz0123456789_-" for character in name):
        raise DirectorSceneDslError("scene DSL tag name is invalid")
    return name, closing, self_closing


def _walk_operation_kinds(operations: list[dict]):
    for operation in operations:
        yield operation["kind"]
        for child_key in ("operations", "events"):
            children = operation.get(child_key)
            if isinstance(children, list):
                yield from _walk_operation_kinds(children)


def _walk_terminations(operations: list[dict]):
    for operation in operations:
        termination = operation.get("termination")
        if isinstance(termination, str):
            yield termination
        for child_key in ("operations", "events"):
            children = operation.get(child_key)
            if isinstance(children, list):
                yield from _walk_terminations(children)


def _hash_json(value: object) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return f"sha256:{sha256(encoded).hexdigest()}"
