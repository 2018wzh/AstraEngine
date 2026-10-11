# AstraEMU

AstraEMU 是同仓的旧 VN 独立 Host。Manager 使用 Slint，Family 以 in-process family plugin 运行；首轮提供 Windows 与 FVP。它不再作为 AstraEngine gameplay runtime provider，不依赖 RuntimeWorld、StateMachine、统一 package/save 或 Host VFS。

Family 持有原生 VM、文件访问、媒体解码、混音、最终画面和原生存档。Host 管理资料库、窗口、物理输入、音频设备、最终帧 HLSL 滤镜、插件与可选翻译服务。一个进程同时只运行一个 session。

保留现有资料库优先的 Slint 布局和元数据功能。游戏原生存档独立于资料库。动态 Family 核心放在程序相邻或用户数据目录的 `cores/`；Manager 冷启动同时扫描并复用 ABI/capability 校验。坏文件单独诊断，跨目录重复 plugin ID 的冲突核心全部拒绝，其他核心继续可用。多个 probe 命中由用户选择。不可用游戏目录保留资料库信息，不阻止客户端启动。

翻译 service 与配置页本轮实现，FVP 声明不支持正文替换，因此相关操作禁用。端到端翻译等待 Musica，不修改 RFVP 翻译路径。滤镜直接消费 Family 最终帧，提供缩放、锐化、Anime4K Restore_S/Upscale_S 和外部 Magpie format 4 HLSL 的明确兼容子集。

FVP、Musica、Artemis、Siglus、CMVS 已进入独立 Emulator workspace；实际能力和验收范围见实施状态。KrKr 的 Family/原生 child 已实现，仍等待 Windows Sandbox 原生流程验收，未作为活动核心接入。BGI、SoftPAL 保留为研究材料。

## 设计与实施入口

- [共享 ABI 与 Host 契约](../contracts/astraemu-ipc.md)
- [实施方案与测试范围](../migrations/astraemu-independent-host.md)
- [实施状态](../status/implementation-plan.md)
- [Family 研究索引](../emu/README.md)
- [Manager 使用手册](../manual/astraemu-manager-metadata.md)
- [客户端运行包与使用](../manual/astraemu-client.md)
