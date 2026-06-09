# Repository Guidelines

## Project Structure & Module Organization

AstraEngine currently has executable Phase 1-4 foundation slices for a modular 2D engine rewrite. Current source-of-truth documents live in `docs/design` and `docs/adr`; user-facing manual scaffolding and implemented foundation guides live in `docs/manual`.

Runtime code lives under `Engine/Runtime`:

- `Core`: diagnostics, diagnostic code registry, release policy, logging, errors, config, path, time, assertions, serialization, stable IDs, build info.
- `Platform`: headless and SDL3-backed platform services behind engine interfaces, opaque dynamic library handles, thread/file-watch/crash foundation services.
- `ModuleRuntime`: plugin descriptors, module loading, `ServiceRegistry`, `ExtensionRegistry`, engine module provider registry, C ABI, service resolve audit, module release-gate evidence.
- `PropertySystem`: property descriptors, flags, type registry, nested JSON schema generation, schema version graph, write policy, migration helpers.
- `Scene`, `Runtime`, `Asset`, `Media`, `Script`, and `AstraVN`: implemented foundation slices only; production completion remains planned in `docs/design/TODO.md`.

Example plugins belong in `Engine/Plugins/Examples`, programs in `Engine/Programs`, samples in `Samples`, and tests in `Engine/Tests`. Development docs are in `docs/development` when introduced; architecture docs are in `docs/design`. Build output should stay in `build/` and is not source.

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

Run the Phase 0 documentation check:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/doc-check.ps1
```

## Coding Style & Naming Conventions

Use C++23 and the repository `.clang-format`. Public headers should live in `Public/Astra/<ModuleName>/`; implementations belong in `Private/`. CMake targets use `Astra_<ModuleName>` for libraries, `Astra_<Name>` for tests, and plain executable names for future programs such as `astra`.

Implementation style is modern C++23 / std-first engine code, not UE C++ clone. Prefer standard library containers, views, filesystem, chrono, RAII ownership, focused DTOs, stable IDs, explicit service interfaces, and Astra `Result`/diagnostics. Do not introduce UE-like replacements such as `TArray`, `TMap`, `FString`, `FName`, `UObject`, `UCLASS`, `UPROPERTY`, GC semantics, or macro-driven public object models.

Use UE-class engines only as an engineering benchmark for lifecycle, tooling, diagnostics, release gates, module policy, and editor/runtime separation. Do not copy UE surface style unless an Astra-specific design document explicitly calls for an equivalent concept implemented through Astra descriptors or services.

Keep Phase 1 modules independent of future VN, AI, Scene, Renderer, or Legacy VM concepts. Public ABI headers must not expose STL ownership, C++ Actor pointers, SDL types, renderer handles, audio handles, OS handles, or editor widgets.

For new C++ APIs, use `PascalCase` for types, `camelCase` for functions and variables, `kPascalCase` for constants, and `PascalCase` enum values. C ABI struct fields and callback names may use lower snake case. Existing early PascalCase member functions may be migrated when API churn is acceptable; do not add new PascalCase C++ methods by default.

## Testing Guidelines

Tests will use Catch2 when runtime code returns. Add focused tests for any new Core, Platform, ModuleRuntime, or PropertySystem behavior. Test names should describe behavior, for example `ServiceRegistry enforces required permissions`.

For plugin work, include at least one integration path through `ModuleManager` or a smoke program once those systems exist.

## Commit & Pull Request Guidelines

Recent history is minimal (`Refactor`, `[docs] ...`). Prefer short imperative commit messages, optionally scoped:

- `[docs] Update development guide`
- `[runtime] Add service registry validation`
- `Refactor plugin descriptor parsing`

Pull requests should include a concise summary, verification commands run, and any architecture or ABI impact. Link issues when available. UI screenshots are only needed for future editor/frontend work.

## Agent-Specific Instructions

Do not revive deleted legacy targets such as `AstraRuntime`, `VNRuntimeServices`, `Bootstrap`, or `AstraGame`. Keep documentation honest about what is implemented now versus planned for later phases.

When adding runtime code, preserve the std-first style from `docs/coding-style.md`: use standard C++ and Astra DTO/handle/descriptor boundaries, keep backend/vendor types private, and avoid UE-style macro/object-system imitation. If a design needs reflection, editor metadata, module lifecycle, or provider registration, implement it through Astra `PropertySystem`, diagnostics, registries, C ABI, or explicit service interfaces rather than hidden macros or engine-object inheritance.
