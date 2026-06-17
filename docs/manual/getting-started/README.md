# Getting Started

Status: NativeVN runtime evidence plus Phase 6 Asset Pipeline and Phase 7 Media Backend evidence. The build baseline, dynamic-only engine DLLs, `astra` CLI, foundation samples, binary `.astrapkg` packaging, media provider/decode/timeline/filter evidence, and NativeVN validate/cook/package/run/replay/inspect workflow exist. Production Editor, AI/MCP, Legacy, and per-driver visual/audio diff remain planned.

## Overview

This section explains how to set up the current repository and run the current foundation/runtime evidence workflow. It also marks the later creator workflow boundaries that are not implemented yet.

## Key Concepts

- Current repository contents include documentation, ADRs, CMake/vcpkg baseline, dynamic `Astra*` runtime/tool libraries, runtime foundation modules, tests, samples, and documentation checks.
- `Samples/NativeVN` currently provides a headless playable Script/AstraVN slice plus source asset sidecars, binary package payload evidence, local DDC evidence, package-only save/replay smoke, and golden replay comparison.
- Future creator flow remains `Template -> Project -> Content -> PIE -> Package`; PIE and Editor workflows are not implemented.

## Architecture

The current build root is `CMakeLists.txt`; shared CMake helpers are in `cmake/AstraTargets.cmake`. Target architecture is in [roadmap](../../design/roadmap.md) and [tools/release/observability](../../design/tools-release-observability.md).

## Programming Guide

Configure and build the baseline:

```powershell
cmake -S . -B build
cmake --build build --config Debug
ctest --test-dir build -C Debug --output-on-failure
build\Bin\astra.exe doc-check
```

Run the current NativeVN evidence chain:

```powershell
build\Bin\astra.exe validate Samples\NativeVN --strict --json
build\Bin\astra.exe cook Samples\NativeVN --config Release --json
build\Bin\astra.exe package Samples\NativeVN --profile deterministic --json
build\Bin\astra.exe run build\Saved\Packages\NativeVN.astrapkg --headless-smoke --json
build\Bin\astra.exe replay build\Saved\Replays\NativeVNGolden.replay --compare --json
build\Bin\astra.exe inspect build\Saved\Packages\NativeVN.astrapkg --json
```

These commands prove the current binary `.astrapkg` runtime evidence workflow, including Phase 7 media provider/decode/timeline/filter evidence. They do not prove Editor workflows, AI/MCP, per-driver visual/audio diff, or final UE-class acceptance.

## API Reference

Current command/API entry points are indexed under [API](../api/README.md), including dynamic-library export headers, Tools DTOs, package/replay evidence DTOs, media capability reports, and foundation runtime APIs.

## Examples

Current examples include running `astra doc-check`, validating `Samples/NativeVN`, cooking and packaging its source sidecars into a binary `.astrapkg`, launching the generated package in headless smoke mode, comparing the golden replay, and inspecting the package manifest/mount evidence.

## Troubleshooting

- If CMake cannot find third-party packages, configure with the intended vcpkg toolchain.
- If `astra` cannot load engine libraries, confirm the command is run from the build tree with `build\Bin` containing the generated `Astra*.dll` files.
- Treat commands that mention Editor, PIE, AI/MCP, Legacy, or per-driver visual/audio diff as target documentation until those systems are implemented.
- Do not restore legacy launch commands for deleted targets.
