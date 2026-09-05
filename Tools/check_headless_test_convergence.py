#!/usr/bin/env python3
"""Inventory test annotations; the old all-Headless policy is retired.

The filename is retained for the existing CI invocation. Exit zero means that
source inventory succeeded, NOT that tests ran or a Headless host was verified.
Counts are lexical: comments, macros, and target configuration can affect them.
"""
from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCES = ("Engine/Source", "Emulator/Source")
ORDINARY = re.compile(r"^\s*#\[(?:tokio::)?test(?:\([^\]]*\))?\]", re.MULTILINE)
HEADLESS = re.compile(r"^\s*#\[astra_headless_test::(?:tokio_)?test\]", re.MULTILINE)


def inventory(root: Path) -> dict[str, object]:
    counts = {"rust_files": 0, "ordinary_test_annotations": 0, "headless_test_annotations": 0}
    source_roots = [root / name for name in SOURCES if (root / name).is_dir()]
    if not source_roots:
        raise FileNotFoundError("no Engine/Source or Emulator/Source directory")
    for source in source_roots:
        for path in sorted(source.rglob("*.rs")):
            text = path.read_text(encoding="utf-8")
            counts["rust_files"] += 1
            counts["ordinary_test_annotations"] += len(ORDINARY.findall(text))
            counts["headless_test_annotations"] += len(HEADLESS.findall(text))
    return {
        "schema": "astra.test_source_inventory.v2",
        "status": "inventory_only",
        "behavior_verified": False,
        "method": "lexical",
        "inventory": counts,
    }


def main() -> int:
    try:
        report = inventory(ROOT)
    except (OSError, UnicodeError) as error:
        print(json.dumps({"status": "error", "diagnostic": str(error)}, ensure_ascii=False))
        return 1
    print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
