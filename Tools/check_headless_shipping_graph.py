#!/usr/bin/env python3
"""Reject test-only Headless crates from shipping dependency graphs."""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys


ROOTS = ("astra-player", "astra-player-web")
FORBIDDEN = {
    "astra-headless",
    "astra-headless-protocol",
    "astra-headless-test",
    "astra-headless-test-macros",
    "astra-headless-vn-adapter",
    "astra-platform-headless",
    "astra-product-host",
}


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[1]
    completed = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    metadata = json.loads(completed.stdout.decode("utf-8"))
    names = {package["id"]: package["name"] for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    root_ids = [package_id for package_id, name in names.items() if name in ROOTS]
    violations: list[dict[str, str]] = []
    for root_id in root_ids:
        stack = [(root_id, names[root_id])]
        visited: set[str] = set()
        while stack:
            package_id, route = stack.pop()
            if package_id in visited:
                continue
            visited.add(package_id)
            for dependency in nodes[package_id]["deps"]:
                normal = any(
                    kind.get("kind") in (None, "build")
                    for kind in dependency.get("dep_kinds", [])
                )
                if not normal:
                    continue
                dependency_id = dependency["pkg"]
                dependency_name = names[dependency_id]
                next_route = f"{route}->{dependency_name}"
                if dependency_name in FORBIDDEN:
                    violations.append(
                        {"root": names[root_id], "dependency": dependency_name, "route": next_route}
                    )
                stack.append((dependency_id, next_route))
    report = {
        "schema": "astra.headless_shipping_graph_report.v1",
        "status": "pass" if not violations else "blocked",
        "roots": list(ROOTS),
        "forbidden": sorted(FORBIDDEN),
        "violations": violations,
    }
    print(json.dumps(report, sort_keys=True))
    return 0 if not violations else 1


if __name__ == "__main__":
    raise SystemExit(main())
