#!/usr/bin/env python3
"""Validate observability classification and source instrumentation coverage."""

from __future__ import annotations

import json
import sys
import tomllib
from pathlib import Path


EVENT_MARKERS = (
    "tracing::trace!",
    "tracing::debug!",
    "tracing::info!",
    "tracing::warn!",
    "tracing::error!",
    "trace!(",
    "debug!(",
    "info!(",
    "warn!(",
    "error!(",
    "info_span!(",
    "debug_span!(",
    "trace_span!(",
)


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    workspace = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    coverage = json.loads(
        (root / "Docs/status/logging-coverage.json").read_text(encoding="utf-8")
    )
    errors: list[str] = []
    if coverage.get("schema") != "astra.logging_coverage.v1":
        errors.append("coverage schema must be astra.logging_coverage.v1")
    entries = coverage.get("crates", {})
    members = workspace["workspace"]["members"]
    names: set[str] = set()
    for member in members:
        crate_root = root / member
        manifest_text = (crate_root / "Cargo.toml").read_text(encoding="utf-8")
        manifest = tomllib.loads(manifest_text)
        name = manifest["package"]["name"]
        names.add(name)
        entry = entries.get(name)
        if entry is None:
            errors.append(f"{name}: missing coverage classification")
            continue
        status = entry.get("status")
        reason = entry.get("reason", "").strip()
        if not reason:
            errors.append(f"{name}: coverage reason is empty")
        if status == "instrumented":
            if not any(line.lstrip().startswith("tracing") for line in manifest_text.splitlines()):
                errors.append(f"{name}: instrumented crate does not depend on tracing")
            sources = "\n".join(
                path.read_text(encoding="utf-8")
                for path in (crate_root / "src").rglob("*.rs")
            )
            if not any(marker in sources for marker in EVENT_MARKERS):
                errors.append(f"{name}: instrumented crate does not emit an event or span")
        elif status != "not_applicable":
            errors.append(f"{name}: unsupported status {status!r}")
    for stale in sorted(set(entries) - names):
        errors.append(f"{stale}: coverage entry is not a workspace member")
    if errors:
        for error in errors:
            print(f"observability: {error}", file=sys.stderr)
        return 1
    print(f"observability coverage: {len(members)} workspace crates classified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
