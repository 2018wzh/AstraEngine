from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
CHECK_PATHS = [DOCS, ROOT / "AGENTS.md"]
FORBIDDEN = [
    "Retired",
    "TBD",
    "占位",
    "旧 C ABI 主",
]
REQUIRED_COVERAGE_COLUMNS = [
    "Design",
    "Contract",
    "Public API",
    "Data Format",
    "Test Scenario",
    "Release Gate",
    "Manual",
]


def iter_markdown_files() -> list[Path]:
    files = sorted(DOCS.rglob("*.md"))
    files.append(ROOT / "AGENTS.md")
    return files


def check_forbidden(files: list[Path]) -> list[str]:
    errors: list[str] = []
    for path in files:
        text = path.read_text(encoding="utf-8")
        for word in FORBIDDEN:
            if word in text:
                errors.append(f"{path.relative_to(ROOT)} contains forbidden text: {word}")
    return errors


def check_links(files: list[Path]) -> list[str]:
    errors: list[str] = []
    link_re = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
    for path in files:
        text = path.read_text(encoding="utf-8")
        for match in link_re.finditer(text):
            target = match.group(1).strip()
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            target = target.split("#", 1)[0]
            if not target:
                continue
            resolved = (path.parent / target).resolve()
            if not resolved.exists():
                errors.append(
                    f"{path.relative_to(ROOT)} links to missing file: {match.group(1)}"
                )
    return errors


def check_coverage_matrix() -> list[str]:
    path = DOCS / "status" / "coverage-matrix.md"
    text = path.read_text(encoding="utf-8")
    errors = [
        f"coverage matrix missing column: {column}"
        for column in REQUIRED_COVERAGE_COLUMNS
        if column not in text
    ]
    module_rows = [line for line in text.splitlines() if line.startswith("| ") and " | " in line]
    # Header, separator, and at least nine module rows.
    if len(module_rows) < 11:
        errors.append("coverage matrix does not list all required module rows")
    return errors


def main() -> int:
    files = iter_markdown_files()
    errors = []
    errors.extend(check_forbidden(files))
    errors.extend(check_links(files))
    errors.extend(check_coverage_matrix())
    if errors:
        for error in errors:
            print(error)
        return 1
    print(f"checked {len(files)} markdown files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
