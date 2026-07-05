# ADR 0002: Editor 使用 Qt/QML + Rust core

## Context

Editor 需要 dock、Inspector、Timeline、Graph、Content Browser、PIE 和 Release Gate UI。复杂桌面工具的原生控件、输入法和多窗口体验很重要。

## Decision

AstraEditor v1 使用 Qt/QML shell + Rust core bridge。Runtime、PIE 和 Debugger 通过 public API 接入，不让 Qt 对象进入 Runtime。

## Consequences

Editor 能较快达到生产工具体验。Rust core 仍是唯一运行时权威，packaged runtime 不依赖 Qt。
