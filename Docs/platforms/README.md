# Platform Specs

平台模块只适配原生能力，不拥有引擎状态。硬目标平台是 Windows、Linux、macOS、iOS、Android、Web；实验平台包括 PSP、PS2、PS3、PSV、3DS、Wii、WiiU、UEFI 等。

| 文档 | 内容 |
| --- | --- |
| [desktop.md](desktop.md) | Windows/Linux/macOS |
| [mobile.md](mobile.md) | iOS/Android |
| [web.md](web.md) | Web/WASM/WebGPU/WebCodecs |
| [experimental.md](experimental.md) | 旧主机/掌机实验模块 |

六平台 host contract、Target binding、capability report 和 profile gate 见 [Target And Platform Blueprint](../implementation/target-platform.md) 与 [Platform Host Blueprint](../implementation/platform-host.md)。Migration 8 当前为 `IN_PROGRESS`：Windows real host 与 Web canvas/WebGPU 基础已落地，但同 commit 完整 conformance/Player evidence 尚未通过；Linux、macOS、iOS、Android 使用显式 `Unavailable` factory 并留在 [Stage 6 Platform Completion](../status/stages/stage-6-platform-completion.md)。

Headless 不属于第七个发布平台。[Migration 11](../migrations/headless-platform-test-backend-migration.md) 已实现独立 `HostKind`、测试 profile 和 `publish = false` 后端，用于平台无关测试与真实平台验收前置；当前状态为 `IN_PROGRESS`，不能计入六平台完成度或替代 E3。
