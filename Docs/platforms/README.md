# Platform Specs

平台模块只适配原生能力，不拥有引擎状态。硬目标平台是 Windows、Linux、macOS、iOS、Android、Web；实验平台包括 PSP、PS2、PS3、PSV、3DS、Wii、WiiU、UEFI 等。

| 文档 | 内容 |
| --- | --- |
| [desktop.md](desktop.md) | Windows/Linux/macOS |
| [mobile.md](mobile.md) | iOS/Android |
| [web.md](web.md) | Web/WASM/WebGPU/WebCodecs |
| [experimental.md](experimental.md) | 旧主机/掌机实验模块 |

六平台 host trait、Target binding、capability report 和 profile gate 见 [Target And Platform Blueprint](../implementation/target-platform.md) 与 [Platform Host Blueprint](../implementation/platform-host.md)。当前实现状态是 Windows repair 已落地；Linux、macOS、iOS、Android 和 Web 只登记 Stage 2 缺口计划，不能作为已完成平台发布。
