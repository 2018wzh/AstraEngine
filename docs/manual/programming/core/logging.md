# Logging Guide

Status: Production logging implemented; trace export and crash bundles remain planned.

## Overview

Astra logs are structured time-series observations for debugging runtime behavior. Diagnostics remain the machine-readable problem reports used by release gates.

## Key Concepts

- Public code uses `Astra/Core/Logging.hpp`; `spdlog` is a private Core dependency.
- `LogEvent` serializes as `astra.log.event.v1` JSONL with channel, component, level, message, objects, fields, source, and optional diagnostic/frame/package/module/asset IDs.
- Default tool logging writes both a human-readable console stream and rotating JSONL files under `build/Saved/Logs`.
- Blocking and fatal diagnostics are mirrored into logs with `diagnostic_code`, but logs do not decide release status.

## Architecture

Core owns logger configuration and sinks. Runtime, Platform, ModuleRuntime, Asset, Media, Script, AstraVN, and Tools write through the process default logger so public APIs do not grow logging parameters for existing foundation workflows.

## Programming Guide

Configure tools with:

```powershell
build\Bin\astra.exe validate Samples\NativeVN --strict --json --log-dir build\Saved\Logs --log-level trace
```

Use channels such as `runtime.event`, `asset.cook`, `media.decode`, `module.lifecycle`, and `tools.lifecycle`. Put high-volume payloads behind `debug` or `trace`; keep `info` suitable for console lifecycle messages.

For tests, configure `LogConfig` with `capture_memory = true`, call `ConfigureLogging()`, then `FlushLogs()` before reading files. Use `ResetLoggingForTests()` when a test installs a custom logger.

## API Reference

Implemented API surface:

- `LogLevel`
- `LogEvent`
- `LogConfig`
- `Logger`
- `DefaultLogger()`
- `ConfigureLogging()`
- `FlushLogs()`
- `ResetLoggingForTests()`
- `LogDiagnostic()`

## Examples

```cpp
Astra::Core::DefaultLogger().Log(
    "runtime.event",
    "event_bus",
    Astra::Core::LogLevel::Debug,
    "runtime event emitted",
    {{"type", event.type.ToString()}});
```

## Troubleshooting

- Use diagnostics for actionable failures and release blocking.
- Use logs for temporal context: lifecycle, timing-adjacent counts, hashes, selected providers, package paths, frame/event IDs.
- Do not include secrets or native handles in log fields.


