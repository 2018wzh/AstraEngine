# AstraEMU Artemis

本目录记录 AstraEMU 的 Artemis family 设计输入和实施口径。资料来自 PFS 公开研究实现、Artemis 官方开发文档镜像，以及合法安装样本的文件级观察。这里不保存商业 payload、剧情正文、截图、音频、视频帧，也不写绕过访问控制的步骤。

## 阅读顺序

| 页面 | 内容 |
| --- | --- |
| [source-inventory.md](source-inventory.md) | 参考源码、官方文档镜像、样本清单和可用事实 |
| [archive-format.md](archive-format.md) | PF6/PF8 archive 结构、索引、加密和 loose movie 边界 |
| [pfs-patch-chain.md](pfs-patch-chain.md) | 根包选择、`.pfs.000` 到 `.pfs.999` patch 顺序和 AstraEMU resolver |
| [script-format.md](script-format.md) | `.iet` text script、`.ast` Lua table script、`.asb` binary script 和辅助表 |
| [script-execution.md](script-execution.md) | `system.ini` boot、tag 执行、Lua 调用和等待点 |
| [script-tags-lua.md](script-tags-lua.md) | `lua`、`calllua`、`tag`、`setTagFilter`、`enqueueTag` 的互操作 |
| [presentation-and-media.md](presentation-and-media.md) | layer、transition、message、BGM/SE/voice/movie 映射 |
| [runtime-core-design.md](runtime-core-design.md) | Artemis family plugin 的边界、状态机、provider 输出和 sandbox |
| [game-observations.md](game-observations.md) | 两个本地样本的本地结构化统计和启动链观察 |
| [tooling.md](tooling.md) | 现有 probe、PFS 工具和下一步 fixture 边界 |
| [implementation-checklist.md](implementation-checklist.md) | 最小实施清单、状态和验收口径 |

## 范围

Artemis 在 AstraEMU 中是 engine-native family plugin。Plugin 持有 PFS resolver、Artemis script VM、Lua sandbox、legacy tag executor、media state 和 save snapshot；Manager 创建 RuntimeWorld 并通过 provider/action/report 接收本地结构化 trace、`PresentationCommand`、`AudioCommand`、`TextCaptureEvent` 和 `StateMachineTrace`。

Artemis 不改变 EngineCore 的 Actor/Component + StateMachine 权威模型，也不把 Artemis 的 tag、Lua 环境、PFS patch 规则或平台视频限制变成公共 Runtime contract。

## 样本基线

本目录统一把本地安装目录写成 `<game-root>`。样本观察只保留：

- PFS 文件名、格式、大小、entry 数量、扩展名分布和少量 header magic。
- `system.ini` 的 boot、分辨率、编码、save path 规则和平台段落。
- `.iet`、`.ast`、`.asb` 的 tag 名、文件名、大小和非剧情 metadata。

不能保留：

- 剧情正文、完整脚本、图片、音频、视频帧、可执行文件、第三方 DLL 或商业补丁内容。
- 任何修改商业 payload、绕过 DRM、绕过授权或绕过访问控制的说明。
