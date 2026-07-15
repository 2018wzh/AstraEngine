"""Strict line-oriented parser for ProjectorRays Director Lingo source."""

from __future__ import annotations

from collections import Counter
from hashlib import sha256
import json
from pathlib import Path
import re


class DirectorLingoError(ValueError):
    """Raised when decompiled Lingo cannot be classified exactly."""


def build_lingo_ir(work_root: Path, converted_resources: dict) -> tuple[dict, dict]:
    if converted_resources.get("schema") != "tsuinosora.projectorrays_converted_resources.v1":
        raise DirectorLingoError("converted resource schema is invalid")
    scripts: list[dict] = []
    statement_counts: Counter[str] = Counter()
    encoding_counts: Counter[str] = Counter()
    handler_count = 0
    source_line_count = 0
    resources = [
        resource
        for resource in converted_resources.get("resources", [])
        if resource.get("chunk_fourcc") == "Lscr"
    ]
    for resource in resources:
        method = resource.get("conversion_method")
        if method == "projectorrays_lscr_empty_script_metadata":
            scripts.append(_empty_script(resource))
            continue
        if method not in {
            "projectorrays_lscr_decompiled_script",
            "projectorrays_lscr_assembly_listing",
        }:
            raise DirectorLingoError("Lscr conversion method is unsupported")
        relative = resource.get("native_path")
        if not isinstance(relative, str) or not relative:
            raise DirectorLingoError("Lscr converted source path is invalid")
        path = (work_root / relative).resolve()
        if work_root.resolve() not in path.parents or not path.is_file():
            raise DirectorLingoError("Lscr converted source path escaped or is missing")
        payload = path.read_bytes()
        expected_hash = resource.get("script_source_sha256")
        actual_hash = f"sha256:{sha256(payload).hexdigest()}"
        if expected_hash != actual_hash:
            raise DirectorLingoError("Lscr converted source hash does not match its evidence")
        text, encoding = _decode_source(payload)
        encoding_counts[encoding] += 1
        parser = _LingoParser(text)
        try:
            parsed = parser.parse()
        except DirectorLingoError as exc:
            source_identity = resource.get("source_relative_path", "unknown")
            raise DirectorLingoError(f"Lscr source {source_identity}: {exc}") from exc
        handler_count += len(parsed["handlers"])
        source_line_count += parsed["source_line_count"]
        statement_counts.update(parsed["statement_counts"])
        scripts.append(
            {
                "source_alias": resource.get("source_alias"),
                "source_relative_path": resource.get("source_relative_path"),
                "source_sha256": resource.get("source_sha256"),
                "cast_library_id": resource.get("cast_library_id"),
                "cast_member_id": resource.get("cast_member_id"),
                "script_number": resource.get("script_number"),
                "script_source_sha256": actual_hash,
                "script_source_kind": resource.get("script_source_kind"),
                "encoding": encoding,
                **parsed,
            }
        )
    if len(scripts) != len(resources):
        raise DirectorLingoError("Lscr source coverage is incomplete")
    detailed = {"schema": "tsuinosora.director_lingo_ir.v1", "scripts": scripts}
    report = {
        "schema": "tsuinosora.director_lingo_report.v1",
        "status": "pass",
        "source_resource_count": len(resources),
        "converted_resource_count": len(scripts),
        "handler_count": handler_count,
        "source_line_count": source_line_count,
        "encoding_counts": dict(sorted(encoding_counts.items())),
        "statement_counts": dict(sorted(statement_counts.items())),
        "lingo_ir_sha256": _hash_json(detailed),
        "diagnostics": [],
        "redaction": {
            "paths": "alias_or_report_relative_only",
            "payload": "omitted",
            "commercial_text": "private_ir_only",
            "script_source": "private_ir_only",
        },
    }
    return detailed, report


def _empty_script(resource: dict) -> dict:
    return {
        "source_alias": resource.get("source_alias"),
        "source_relative_path": resource.get("source_relative_path"),
        "source_sha256": resource.get("source_sha256"),
        "cast_library_id": resource.get("cast_library_id"),
        "cast_member_id": resource.get("cast_member_id"),
        "script_number": resource.get("script_number"),
        "script_source_sha256": None,
        "script_source_kind": "empty",
        "encoding": "none",
        "source_line_count": 0,
        "declarations": [],
        "handlers": [],
        "statement_counts": {},
    }


class _LingoParser:
    def __init__(self, source: str) -> None:
        self.lines = [line.strip() for line in source.splitlines() if line.strip()]

    def parse(self) -> dict:
        declarations: list[dict] = []
        handlers: list[dict] = []
        counts: Counter[str] = Counter()
        current: dict | None = None
        stack: list[str] = []
        for number, line in enumerate(self.lines, start=1):
            lower = line.lower()
            if lower.startswith(("global ", "property ")):
                kind, value = line.split(None, 1)
                declaration = {
                    "kind": kind.lower(),
                    "names": [_identifier(item.strip(), number) for item in value.split(",")],
                }
                if current is None:
                    declarations.append(declaration)
                else:
                    current["statements"].append(
                        {"kind": "declaration", "scope": declaration["kind"], "names": declaration["names"]}
                    )
                    counts["declaration"] += 1
                continue
            if lower.startswith("on ") or lower.startswith("macro "):
                if current is not None or stack:
                    raise DirectorLingoError(f"nested handler at line {number}")
                header = re.fullmatch(r"(on|macro)\s+([A-Za-z_][A-Za-z0-9_]*)(?:\s+(.*))?", line, re.IGNORECASE)
                if header is None:
                    raise DirectorLingoError(f"invalid handler declaration at line {number}")
                parameters = [item for item in re.split(r"[\s,]+", header.group(3) or "") if item]
                current = {
                    "kind": header.group(1).lower(),
                    "name": _identifier(header.group(2), number),
                    "parameters": [_identifier(item, number) for item in parameters],
                    "statements": [],
                }
                continue
            if current is None:
                raise DirectorLingoError(f"statement outside handler at line {number}")
            if lower == "end":
                if stack:
                    raise DirectorLingoError(f"handler ended with open {stack[-1]} block at line {number}")
                handlers.append(current)
                current = None
                continue
            statement = _parse_statement(line, number)
            kind = statement["kind"]
            if kind in {"if_begin", "case_begin", "repeat_begin", "tell_begin"}:
                stack.append(kind.removesuffix("_begin"))
            elif kind in {"if_end", "case_end", "repeat_end", "tell_end"}:
                expected = kind.removesuffix("_end")
                if not stack or stack[-1] != expected:
                    raise DirectorLingoError(f"mismatched {kind} at line {number}")
                stack.pop()
            elif kind == "else" and (not stack or stack[-1] != "if"):
                raise DirectorLingoError(f"else outside if block at line {number}")
            elif kind in {"case_label", "case_otherwise"} and (not stack or stack[-1] != "case"):
                raise DirectorLingoError(f"case label outside case block at line {number}")
            current["statements"].append(statement)
            counts[kind] += 1
        if current is not None or stack:
            raise DirectorLingoError("Lingo source ended with an open handler or block")
        if not handlers:
            raise DirectorLingoError("non-empty Lingo source contains no handlers")
        return {
            "source_line_count": len(self.lines),
            "declarations": declarations,
            "handlers": handlers,
            "statement_counts": dict(sorted(counts.items())),
        }


def _parse_statement(line: str, number: int) -> dict:
    lower = line.lower()
    if lower.startswith("if ") and lower.endswith(" then"):
        return {"kind": "if_begin", "condition": _tokens(line[3:-5], number)}
    if lower == "else":
        return {"kind": "else"}
    if lower == "end if":
        return {"kind": "if_end"}
    if lower.startswith("case ") and lower.endswith(" of"):
        return {"kind": "case_begin", "expression": _tokens(line[5:-3], number)}
    if lower == "otherwise:":
        return {"kind": "case_otherwise"}
    if lower == "end case":
        return {"kind": "case_end"}
    if lower.startswith("repeat "):
        return {"kind": "repeat_begin", "expression": _tokens(line[7:], number)}
    if lower == "end repeat":
        return {"kind": "repeat_end"}
    if lower.startswith("tell "):
        return {"kind": "tell_begin", "target": _tokens(line[5:], number)}
    if lower == "end tell":
        return {"kind": "tell_end"}
    if lower.startswith("return") and (len(line) == 6 or line[6].isspace()):
        return {"kind": "return", "value": _tokens(line[6:].strip(), number)}
    if lower in {"exit", "exit repeat", "next repeat"}:
        return {"kind": lower.replace(" ", "_")}
    if lower.startswith("go") and (len(line) == 2 or not line[2].isalnum()):
        return {"kind": "go", "expression": _tokens(line[2:].strip(), number)}
    if lower.startswith("set "):
        return {"kind": "set", "expression": _tokens(line[4:], number)}
    if lower.startswith("put "):
        return {"kind": "put", "expression": _tokens(line[4:], number)}
    if line.endswith(":"):
        return {"kind": "case_label", "value": _tokens(line[:-1], number)}
    tokens = _tokens(line, number)
    if any(token["kind"] == "operator" and token["value"] == "=" for token in tokens):
        return {"kind": "assignment", "expression": tokens}
    return {"kind": "command", "expression": tokens}


TOKEN_RE = re.compile(
    r'''\s*(?:("(?:[^"]|"")*")|(#?[A-Za-z_][A-Za-z0-9_]*)|(\d+(?:\.\d+)?)|(\.\.|<=|>=|<>|=|\+|-|\*|/|&|<|>)|([\[\](),.:]))'''
)


def _tokens(source: str, number: int) -> list[dict]:
    if not source:
        return []
    tokens: list[dict] = []
    cursor = 0
    while cursor < len(source):
        match = TOKEN_RE.match(source, cursor)
        if not match:
            raise DirectorLingoError(f"unrecognized Lingo token at line {number}")
        string, identifier, number_value, operator, punctuation = match.groups()
        if string is not None:
            tokens.append({"kind": "string", "value": string[1:-1].replace('""', '"')})
        elif identifier is not None:
            tokens.append(
                {"kind": "symbol" if identifier.startswith("#") else "identifier", "value": identifier}
            )
        elif number_value is not None:
            tokens.append({"kind": "number", "value": number_value})
        elif operator is not None:
            tokens.append({"kind": "operator", "value": operator})
        else:
            tokens.append({"kind": "punctuation", "value": punctuation})
        cursor = match.end()
    return tokens


def _identifier(value: str, number: int) -> str:
    if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", value):
        raise DirectorLingoError(f"invalid Lingo identifier at line {number}")
    return value


def _decode_source(payload: bytes) -> tuple[str, str]:
    try:
        return payload.decode("utf-8"), "utf-8"
    except UnicodeDecodeError:
        try:
            return payload.decode("cp932"), "cp932"
        except UnicodeDecodeError as exc:
            raise DirectorLingoError("Lingo source is neither UTF-8 nor CP932") from exc


def _hash_json(value: object) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return f"sha256:{sha256(encoded).hexdigest()}"
