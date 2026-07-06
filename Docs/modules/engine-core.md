# EngineCore Module

EngineCore 是所有产品线共享的引擎内核。它提供 RuntimeWorld、Actor/Component、StateMachine、PropertySystem、diagnostics、schema、migration、plugin loading 和 test harness。

## Crate 边界

| Crate | 职责 |
| --- | --- |
| `astra-core` | stable id、diagnostics、schema、migration、hash、result 类型 |
| `astra-runtime` | RuntimeWorld、Actor/Component、StateMachine、EventBus、Scheduler、AwaitToken |
| `astra-plugin` | `abi_stable` 风格插件 ABI、descriptor、loader、registry |
| `astra-property` | derive 宏、PropertySystem metadata、Inspector/MCP/save glue |
| `astra-test` | headless runner、scenario engine、report comparison |
| `astra-cli` | Stage 1 CLI：`astra test run`、`astra report explain` |

Stage 1 代码位于 UE 风格 workspace：`Engine/Source/Runtime/`、`Engine/Source/Developer/`、`Engine/Source/Programs/` 和 `Engine/Plugins/Fixtures/`。

## 实现顺序

1. `astra-core` 定义 StableId、Diagnostic、SchemaVersion、Hash128。
2. `astra-runtime` 实现固定 tick、事件队列、同步 guard、AwaitToken action。
3. `astra-save` 或 runtime 内 section writer 实现自描述二进制 save。
4. `astra-plugin` 实现 descriptor validation 和加载/卸载。
5. `astra-test` 跑 YAML scenario 并生成 report。

## 验收

同一 package、seed、input、committed AI output 重跑两次，state/event/presentation hash 必须一致。Save 后 load 再 replay，hash mismatch 必须定位 frame、event、actor/component、script command 或 provider output。

## 产品级交付面

- `astra-core` 提供 StableId、SchemaId、SourceSpan、Diagnostic、Hash128/Hash256、schema registry 和 migration registry。
- `astra-runtime` 提供 RuntimeWorld lifecycle、Actor/Component store、StateMachineStore、EventQueue、AwaitQueue、MutationLog、Save/Replay facade 和 RuntimeDebugSession。
- `astra-plugin` 提供 descriptor gate、provider registry、EngineModuleSlot 和 load/unload report。
- `astra-property` 提供 derive metadata、Inspector field model、serde/schema glue 和 `cargo expand` 可读输出。
- `astra-test` 提供 YAML scenario runner、hash compare、report writer 和 replay mismatch explain。

实现字段和 trait 见 [Runtime API Blueprint](../implementation/runtime-api.md)、[Provider And Plugin API Blueprint](../implementation/provider-plugin-api.md)。
