# Architecture Decision Records

本目录记录 AstraEngine 当前目标架构决策。旧结论已按“模块化 2D 引擎，VN 为第一垂直模块”的方向直接重写。

| ADR | 决策 |
| --- | --- |
| [0001](0001-modular-2d-engine-baseline.md) | 模块化 2D 引擎基线 |
| [0002](0002-editor-ui-qt.md) | 第一阶段 Editor UI 使用 Qt dockable shell |
| [0003](0003-renderer2d-first-stage-backend.md) | 第一阶段 Renderer2D 后端 |
| [0004](0004-actor-component-statemachine-runtime.md) | Actor/Component + StateMachineRuntime，ECS 仅用于热点 |
| [0005](0005-mcp-agent-capability-protocol.md) | MCP 是 Agent 能力协议层 |
| [0006](0006-text-first-source-data.md) | Text-First Source Data |
| [0008](0008-dynamic-modules-service-registry-c-abi.md) | 动态模块优先、ServiceRegistry、C ABI |
| [0009](0009-legacy-vn-emulator-modernization.md) | Legacy VN 模拟器和现代化插件 |
| [0010](0010-actor-component-statemachine-core.md) | Core 承载 Actor/Component/StateMachine |
