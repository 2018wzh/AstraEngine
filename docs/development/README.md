# 开发文档

状态：当前开发文档描述的是 **已落地的 Phase 1 工程基线**，不是完整目标架构实现。

当前代码主线只覆盖：

- `Astra_Core`
- `Astra_Platform`
- `Astra_ModuleRuntime`
- `Astra_PropertySystem`
- `AstraPhase1Smoke`
- `Astra_Phase1Tests`

建议阅读顺序：

1. [building.md](./building.md)
2. [testing.md](./testing.md)
3. [runtime.md](./runtime.md)
4. [plugin-abi.md](./plugin-abi.md)
5. [phase1-smoke.md](./phase1-smoke.md)
6. [yaml-source-format.md](./yaml-source-format.md)

尚未实现：

- Scene / ActorWorld / EventBus runtime integration
- StateMachineRuntime
- ScriptRuntimeHost
- Asset / Media / FilterGraph
- AstraVN
- AI runtime intent
- Legacy VN emulator
