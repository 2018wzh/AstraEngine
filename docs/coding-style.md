# Coding Style

Status: Draft

## C++ Baseline

- Use C++23.
- Prefer the C++ standard library for strings, containers, paths, time, optionals, variants, smart pointers, and `std::expected`.
- Do not introduce engine-specific replacements for standard containers or strings unless an ADR documents the need.

## Naming

- Public CMake targets use the `Astra_` prefix, for example `Astra_Core`.
- Types use `PascalCase`.
- Functions and variables use `snake_case`.
- Constants use `kPascalCase` when they have internal linkage and `PascalCase` for enum values.
- File names should be descriptive and match the main type or module concept.

## Errors and Assertions

- Use `std::expected` for recoverable errors where the caller can reasonably handle failure.
- Use assertions for programmer errors and invalid internal invariants.
- Use fatal error entry points only when continuing would corrupt state or produce misleading output.
- Error codes and messages should identify the failing subsystem and operation.

## Module Boundaries

- Runtime modules must not depend on Editor modules.
- Editor modules may depend on Runtime modules.
- Core must remain free of SDL, rendering, audio, AI provider, and Editor dependencies.
- Runtime plugins must not depend on Editor plugin modules.
- AI output that can affect canonical project content must pass through patch or review queue flows.

## Formatting and Static Checks

- Format C++ and CMake-adjacent source files with the repository `.clang-format`.
- Use `.clang-tidy` as the initial static analysis baseline.
- CI must configure CMake, build, and run CTest before changes can be merged.
