# Getting Started

Status: Phase 0 scaffold. Build baseline exists; runtime projects, samples, packaging, and `astra` CLI are planned.

## Overview

This section explains how to set up the current repository and how the future creator workflow will work once Phase 1+ systems are implemented.

## Key Concepts

- Current repository contents are documentation, ADRs, CMake/vcpkg baseline, and documentation checks.
- Phase 0 does not include `Engine/Runtime`, `Samples`, or a launchable game.
- Future creator flow is `Template -> Project -> Content -> PIE -> Package`.

## Architecture

The current build root is `CMakeLists.txt`; shared CMake helpers are in `cmake/AstraTargets.cmake`. Target architecture is in [roadmap](../../design/roadmap.md) and [tools/release/observability](../../design/tools-release-observability.md).

## Programming Guide

Configure and build the baseline:

```powershell
cmake -S . -B build
cmake --build build --config Debug
ctest --test-dir build -C Debug --output-on-failure
powershell -NoProfile -ExecutionPolicy Bypass -File tools/doc-check.ps1
```

The future project creation and packaging commands will be documented here after the `astra` CLI exists.

## API Reference

No getting-started API is implemented in Phase 0. Future command references will live under [API](../api/README.md) and tool docs.

## Examples

Current example: run `tools/doc-check.ps1` to validate required manual pages, links, and stale legacy references.

Planned examples: create `Samples/NativeVN`, run the sample, cook it, package it, and inspect release evidence.

## Troubleshooting

- If CMake cannot find third-party packages, configure with the intended vcpkg toolchain.
- If a command references `astra`, it is currently target documentation, not a runnable Phase 0 command.
- Do not restore legacy launch commands for deleted targets.
