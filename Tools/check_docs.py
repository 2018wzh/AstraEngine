#!/usr/bin/env python3
"""Check documentation hygiene, not architecture vocabulary or product completion.

The link check covers ordinary inline Markdown links outside fenced code blocks.
It does not fetch external URLs, validate anchors, or certify implementation status.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
LINK = re.compile(r"\[[^\]]*\]\((<[^>]*>|[^\s)]+)(?:\s+[^)]*)?\)")
FENCE = re.compile(r"^\s{0,3}(`{3,}|~{3,})")


def prose_lines(text: str):
    fence = None
    for line in text.splitlines():
        match = FENCE.match(line)
        if match:
            marker = match.group(1)
            if fence is None:
                fence = marker
            elif marker[0] == fence[0] and len(marker) >= len(fence):
                fence = None
            continue
        if fence is None:
            yield line


def check_document(path: Path, root: Path) -> list[str]:
    relative = path.relative_to(root).as_posix()
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        return [f"{relative}: cannot read UTF-8 text: {error}"]
    errors = []
    if any(ord(char) < 32 and char not in "\t\n\r" or ord(char) == 127 for char in text):
        errors.append(f"{relative}: contains unexpected control characters")
    for line in prose_lines(text):
        for match in LINK.finditer(line):
            target = match.group(1).strip("<>")
            try:
                parsed = urlsplit(target)
                if parsed.scheme or parsed.netloc or not parsed.path:
                    continue
                resolved = (path.parent / unquote(parsed.path)).resolve()
                if not resolved.is_relative_to(root) or not resolved.exists():
                    errors.append(f"{relative}: unresolved local link: {target}")
            except (ValueError, OSError) as error:
                errors.append(f"{relative}: invalid local link {target!r}: {error}")
    return errors


def check_repository(root: Path) -> tuple[int, list[str]]:
    root = root.resolve()
    paths = sorted((root / "Docs").rglob("*.md"))
    paths.append(root / "AGENTS.md")
    if (root / "README.md").is_file():
        paths.append(root / "README.md")
    errors = [error for path in paths for error in check_document(path, root)]
    return len(paths), errors


def main() -> int:
    count, errors = check_repository(ROOT)
    for error in errors:
        print(error)
    if not errors:
        print(f"checked {count} Markdown files (UTF-8, control characters, inline local links only)")
    return int(bool(errors))


if __name__ == "__main__":
    sys.exit(main())
