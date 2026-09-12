"""Check local documentation links and accidental private paths.

Architecture/status policy belongs in review, not a list of required words.
The checker intentionally accepts drafts, TODOs and incomplete implementations.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
PRIVATE_PATH = re.compile(r"(?:(?<![A-Za-z0-9_])[A-Za-z]:[\\/]|/(?:home|Users)/[^/\s<>]+/)")
INLINE_LINK = re.compile(r"!?\[[^\]\n]*\]\((<[^>]+>|[^\s)]+)(?:\s+[\"'][^\n]*?[\"'])?\)")
REFERENCE = re.compile(r"^ {0,3}\[[^\]^]+\]:\s*(<[^>]+>|\S+)", re.MULTILINE)


def prose(text: str) -> str:
    """Ignore fenced examples when finding links (not when checking paths)."""
    result = []
    fence = None
    for line in text.splitlines():
        marker = re.match(r"^ {0,3}(`{3,}|~{3,})", line)
        if marker:
            value = marker.group(1)
            if fence is None:
                fence = value
            elif value[0] == fence[0] and len(value) >= len(fence):
                fence = None
            result.append("")
        else:
            result.append(line if fence is None else "")
    return "\n".join(result)


def check_file(path: Path, root: Path) -> list[str]:
    label = path.relative_to(root).as_posix()
    try:
        content = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        return [f"{label}: cannot read UTF-8 document ({type(error).__name__})"]
    errors = []
    if PRIVATE_PATH.search(content):
        errors.append(f"{label}: contains a private absolute path")
    if any(ord(char) < 32 and char not in "\n\r\t" for char in content):
        errors.append(f"{label}: contains a control character")
    text = prose(content)
    for pattern in (INLINE_LINK, REFERENCE):
        for match in pattern.finditer(text):
            target = match.group(1).strip("<>")
            try:
                parsed = urlsplit(target)
            except ValueError:
                errors.append(f"{label}: malformed link")
                continue
            if parsed.scheme or parsed.netloc or not parsed.path:
                continue
            resolved = (path.parent / unquote(parsed.path)).resolve()
            if not resolved.is_relative_to(root.resolve()):
                errors.append(f"{label}: local link escapes repository")
            elif not resolved.exists():
                errors.append(f"{label}: missing link {target}")
    return errors


def check(root: Path) -> tuple[int, list[str]]:
    files = sorted((root / "Docs").rglob("*.md"))
    files.extend(path for name in ("AGENTS.md", "README.md") if (path := root / name).is_file())
    return len(files), [error for path in files for error in check_file(path, root)]


def main() -> int:
    count, errors = check(ROOT)
    for error in errors:
        print(error)
    print(f"Checked {count} documents; {len(errors)} link/hygiene errors.")
    return int(bool(errors))


if __name__ == "__main__":
    sys.exit(main())
