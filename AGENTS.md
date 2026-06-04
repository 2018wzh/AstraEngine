# Repository Guidelines

## Project Structure & Module Organization

AstraEngine is currently in Phase 1 of a modular 2D engine rewrite. Runtime code lives under `Engine/Runtime`:

- `Core`: diagnostics, logging, errors, config, path, time, assertions.
- `Platform`: SDL3-backed platform services behind engine interfaces.
- `ModuleRuntime`: plugin descriptors, module loading, `ServiceRegistry`, `ExtensionRegistry`, C ABI.
- `PropertySystem`: property descriptors, flags, type registry, JSON schema generation.

Example plugins live in `Engine/Plugins/Examples`, programs in `Engine/Programs`, and tests in `Engine/Tests`. Development docs are in `docs/development`; architecture docs are in `docs/design`. Build output should stay in `build/` and is not source.

## Build, Test, and Development Commands

Configure:

```powershell
cmake -S . -B build
```

Build Debug:

```powershell
cmake --build build --config Debug
```

Run tests:

```powershell
ctest --test-dir build -C Debug --output-on-failure
```

Run the Phase 1 smoke program:

```powershell
.\build\Bin\AstraPhase1Smoke.exe
```

The smoke program verifies platform service setup, plugin discovery, module loading, extension registration, and unload.

## Coding Style & Naming Conventions

Use C++23 and the repository `.clang-format`. Public headers should live in `Public/Astra/<ModuleName>/`; implementations belong in `Private/`. CMake targets use `Astra_<ModuleName>` for libraries, `Astra_<Name>` for tests, and plain executable names for programs such as `AstraPhase1Smoke`.

Keep Phase 1 modules independent of future VN, AI, Scene, Renderer, or Legacy VM concepts. Public ABI headers must not expose STL ownership, C++ Actor pointers, SDL types, renderer handles, or editor widgets.

## Testing Guidelines

Tests use Catch2 and currently live in `Engine/Tests/Phase1Tests.cpp`. Add focused tests for any new Core, Platform, ModuleRuntime, or PropertySystem behavior. Test names should describe behavior, for example `ServiceRegistry enforces required permissions`.

For plugin work, include at least one integration path through `ModuleManager` or the smoke program.

## Commit & Pull Request Guidelines

Recent history is minimal (`Refactor`, `[docs] ...`). Prefer short imperative commit messages, optionally scoped:

- `[docs] Update development guide`
- `[runtime] Add service registry validation`
- `Refactor plugin descriptor parsing`

Pull requests should include a concise summary, verification commands run, and any architecture or ABI impact. Link issues when available. UI screenshots are only needed for future editor/frontend work.

## Agent-Specific Instructions

Do not revive deleted legacy targets such as `AstraRuntime`, `VNRuntimeServices`, `Bootstrap`, or `AstraGame`. Keep documentation honest about what is implemented now versus planned for later phases.
