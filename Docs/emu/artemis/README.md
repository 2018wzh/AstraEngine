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
| [MODIFICATIONS](../../../Emulator/Source/Families/astra-emu-artemis/MODIFICATIONS.md) | 当前核心基线、嵌入差异与上游修复 |
| [game-observations.md](game-observations.md) | 两个本地样本的本地结构化统计和启动链观察 |
| [tooling.md](tooling.md) | 现有 probe、PFS 工具和下一步 fixture 边界 |
| [实施计划](../../status/implementation-plan.md) | 当前整改顺序与验收条件 |

## 范围

Artemis Family 直接复用 art3m1s-core 原 VM、Lua 5.1、GPU 和 FFmpeg 9；核心无 PCM mixer，Family 复用 SDK Symphonia/Kira。Manager 通过 [Family API](../../../Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md) 接收不可变 CPU lease、有界 PCM 和 typed 宿主请求，不依赖 RuntimeWorld。

当前问题与证据见[性能审计](../../status/performance-upstream-audit-2026-10-12.md)。onLoad 等待修复已提交 [PR #5](https://github.com/Alphaly2K/art3m1s-core/pull/5)，等待合并；本地功能修复尚未全部上游化。下列格式研究不作为当前实现完成声明。

## 样本基线

本目录统一把本地安装目录写成 `<game-root>`。样本观察只保留：

- PFS 文件名、格式、大小、entry 数量、扩展名分布和少量 header magic。
- `system.ini` 的 boot、分辨率、编码、save path 规则和平台段落。
- `.iet`、`.ast`、`.asb` 的 tag 名、文件名、大小和非剧情 metadata。

不能保留：

- 剧情正文、完整脚本、图片、音频、视频帧、可执行文件、第三方 DLL 或商业补丁内容。
- 任何修改商业 payload、绕过 DRM、绕过授权或绕过访问控制的说明。
