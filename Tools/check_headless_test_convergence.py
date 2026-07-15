#!/usr/bin/env python3
"""Enforce Migration 11's single platform-neutral test lifecycle."""

from __future__ import annotations

import json
import pathlib
import re
import sys


RAW_TEST = re.compile(r"^\s*#\[(?:tokio::)?test\]", re.MULTILINE)
IGNORED_HEADLESS_TEST = re.compile(
    r"#\[astra_headless_test::(?:tokio_)?test\]\s*#\[ignore(?:\s*=\s*[^\]]+)?\]",
    re.MULTILINE,
)
FORBIDDEN_PRODUCT_PATHS = (
    "ScenarioRunner",
    "HeadlessRendererProvider",
    "PlayerPersistentAudioMixer",
)
CONTROLLED = (
    "Engine/Source/Runtime",
    "Engine/Source/Developer",
    "Engine/Source/Modules",
    "Engine/Source/Programs",
)
PLATFORM_TEST_MARKERS = (
    "/tests/windows_",
    "/tests/web_",
    "/astra-player-web/",
    "/astra-crash-reporter/tests/windows_",
    "/src/web_cdp.rs",
)
PLATFORM_DOCTEST_EXEMPT = {
    "Engine/Source/Programs/astra-player-web/Cargo.toml",
}


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[1]
    violations: list[dict[str, object]] = []
    inventory = {
        "rust_files": 0,
        "headless_tests": 0,
        "ignored_headless_tests": 0,
        "platform_exempt_tests": 0,
        "controlled_library_targets": 0,
        "doctest_disabled_targets": 0,
    }
    for relative_root in CONTROLLED:
        for path in sorted((root / relative_root).rglob("*.rs")):
            relative = path.relative_to(root).as_posix()
            text = path.read_text(encoding="utf-8")
            inventory["rust_files"] += 1
            raw = len(RAW_TEST.findall(text))
            headless = text.count("#[astra_headless_test::test]") + text.count(
                "#[astra_headless_test::tokio_test]"
            )
            inventory["headless_tests"] += headless
            inventory["ignored_headless_tests"] += len(IGNORED_HEADLESS_TEST.findall(text))
            if raw:
                if any(marker in "/" + relative for marker in PLATFORM_TEST_MARKERS):
                    inventory["platform_exempt_tests"] += raw
                else:
                    violations.append(
                        {"path": relative, "code": "ASTRA_HEADLESS_RAW_TEST_FORBIDDEN", "count": raw}
                    )
            for symbol in FORBIDDEN_PRODUCT_PATHS:
                if symbol not in text:
                    continue
                violations.append(
                    {"path": relative, "code": "ASTRA_HEADLESS_LEGACY_PATH_FORBIDDEN", "symbol": symbol}
                )
    for relative_root in CONTROLLED:
        for manifest in sorted((root / relative_root).rglob("Cargo.toml")):
            library = manifest.parent / "src" / "lib.rs"
            if not library.is_file():
                continue
            relative = manifest.relative_to(root).as_posix()
            if relative in PLATFORM_DOCTEST_EXEMPT:
                continue
            inventory["controlled_library_targets"] += 1
            text = manifest.read_text(encoding="utf-8")
            if re.search(r"^doctest\s*=\s*false\s*$", text, re.MULTILINE):
                inventory["doctest_disabled_targets"] += 1
            else:
                violations.append(
                    {"path": relative, "code": "ASTRA_HEADLESS_DOCTEST_TARGET_FORBIDDEN"}
                )
    report = {
        "schema": "astra.headless_test_inventory.v1",
        "status": "pass" if not violations else "blocked",
        "inventory": inventory,
        "violations": violations,
    }
    print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0 if not violations else 1


if __name__ == "__main__":
    raise SystemExit(main())
