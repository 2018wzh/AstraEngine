# ADR 0003: Renderer2D 后端可替换，wgpu 为默认 provider

## Context

AstraEngine 要覆盖桌面、移动、Web 和实验平台。单一后端无法长期覆盖所有硬件和平台约束。

## Decision

Renderer2D 通过 EngineModuleSlot 选择 provider。wgpu 是默认 provider；平台或实验后端可以注册替代 provider。Public Media API 不暴露 native GPU handle。

## Consequences

主线实现集中在 wgpu，平台特例留在 provider。Release Gate 必须检查 provider fingerprint、capability 和 packaged eligibility。
