# ADR 0001: 模块化 2D/VN-first 引擎基线

## Context

AstraEngine 需要支撑原生 VN、创作者 Editor、插件扩展、平台发布和旧 VN 兼容。复杂 3D、网络竞技和开放世界不是目标。

## Decision

AstraEngine 以 2D/VN-first 为主线，EngineCore 只承载可复用 runtime、asset、media、script、plugin 和 test contracts。AstraVN、AstraEditor、AstraEMU、AstraPlatform 按系列仓库分离。

## Consequences

Core 保持干净；产品能力通过模块和 provider 接入。旧 VN 兼容不能成为 NativeVN 达标前置条件。
