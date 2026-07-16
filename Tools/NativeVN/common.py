"""Shared, path-safe diagnostics and file helpers for flagship content tools."""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable


SAFE_ID = re.compile(r"^[a-z0-9][a-z0-9._-]{0,95}$")


class ToolFailure(RuntimeError):
    """A stable, user-actionable tool failure."""

    def __init__(self, code: str, message: str, *, path: str | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.path = path


@dataclass(order=True, frozen=True)
class Diagnostic:
    severity: str
    code: str
    message: str
    path: str | None = None

    def as_dict(self) -> dict[str, str]:
        result = {"severity": self.severity, "code": self.code, "message": self.message}
        if self.path is not None:
            result["path"] = self.path
        return result


@dataclass
class Diagnostics:
    items: list[Diagnostic] = field(default_factory=list)

    def add(self, severity: str, code: str, message: str, path: str | None = None) -> None:
        self.items.append(Diagnostic(severity, code, message, path))

    def error(self, code: str, message: str, path: str | None = None) -> None:
        self.add("error", code, message, path)

    def warning(self, code: str, message: str, path: str | None = None) -> None:
        self.add("warning", code, message, path)

    @property
    def failed(self) -> bool:
        return any(item.severity == "error" for item in self.items)

    def emit_json(self, *, summary: dict[str, Any] | None = None) -> None:
        payload: dict[str, Any] = {
            "schema": "astra.nativevn_flagship.diagnostics.v1",
            "diagnostics": [item.as_dict() for item in sorted(self.items)],
            "status": "blocked" if self.failed else "ok",
        }
        if summary is not None:
            payload["summary"] = summary
        print(json.dumps(payload, ensure_ascii=False, sort_keys=True))


def display_path(path: Path, root: Path) -> str:
    """Return a stable relative path and never expose a local absolute path."""
    try:
        value = path.resolve().relative_to(root.resolve()).as_posix()
    except ValueError:
        value = path.name
    return value or "."


def require_within(path: Path, root: Path, code: str = "NATIVEVN_PATH_OUTSIDE_ROOT") -> Path:
    resolved = path.resolve()
    try:
        resolved.relative_to(root.resolve())
    except ValueError as error:
        raise ToolFailure(code, "output path must remain inside the configured root", path=path.name) from error
    return resolved


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json(path: Path) -> Any:
    try:
        with path.open("r", encoding="utf-8") as stream:
            return json.load(stream)
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ToolFailure("NATIVEVN_JSON_INVALID", "JSON file is unreadable or invalid", path=path.name) from error


def write_json_atomic(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    encoded = json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    try:
        temporary.write_text(encoded, encoding="utf-8", newline="\n")
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def require_executable(name: str) -> str:
    executable = shutil.which(name)
    if executable is None:
        raise ToolFailure(
            "NATIVEVN_DEPENDENCY_MISSING",
            f"required executable '{name}' was not found on PATH; install it and retry",
        )
    return executable


def validate_safe_id(value: object, field_name: str) -> str:
    if not isinstance(value, str) or SAFE_ID.fullmatch(value) is None:
        raise ToolFailure("NATIVEVN_ID_INVALID", f"{field_name} must be a lowercase safe identifier")
    return value


def iter_files(root: Path, *, excluded_parts: Iterable[str] = ()) -> Iterable[Path]:
    excluded = set(excluded_parts)
    for path in sorted(root.rglob("*")):
        if path.is_file() and not any(part in excluded for part in path.relative_to(root).parts):
            yield path
