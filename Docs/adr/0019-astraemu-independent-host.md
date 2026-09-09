# ADR 0019: AstraEMU 独立 Host

Status: Accepted，2026-09-08。

## 背景

旧 Family 接入需要经过 product runtime、RuntimeWorld、StateMachine、VFS、Hook、Layer2D 和多套生命周期，迫使原生引擎拆开原有渲染、媒体与存档路径。用户决定重新划分边界，以第三方容易接入和保持游戏行为为目标。

## 决策

AstraEMU 留在同一仓库，使用独立轻量 Host。Slint Manager 持有窗口、输入、音频设备和最终帧滤镜；in-process Family 持有完整 VM、原生文件、解码、混音、CPU 最终帧和游戏原生存档。一个进程只运行一个 session。

独立 `abi_stable` Family API 是第三方唯一必需依赖。删除旧 RuntimeWorld/product provider、VFS、通用 Hook、Trusted Luau patch 与统一 package/save 路径，不保留兼容层。首轮 Windows/FVP，其他 Family 源码保留但暂不接入。

可选翻译是异步 typed 正文服务，失败由 Family 保留原文并报告；FVP 本轮不支持此能力。滤镜使用独立 Magpie format 4 HLSL 兼容层、固定 DXC、Naga/wgpu，并从 MIT Anime4K 源码移植内置效果。

## 影响

此决策取代 [ADR 0012](0012-astraemu-engine-native-family-plugin.md) 的 AstraEMU 架构。AstraVN、Editor、EngineCore 的 RuntimeWorld 和平台契约继续有效。Manager 资料库与设置采用新 schema，原生游戏存档仍归 Family。

具体生命周期、错误、缓存和测试范围以 [实施方案](../migrations/astraemu-independent-host.md) 和 [共享契约](../contracts/astraemu-ipc.md) 为准；实施进度见 [Stage 5](../status/stages/stage-5-astra-emu.md)。
