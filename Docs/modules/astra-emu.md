# AstraEMU

AstraEMU 是同仓的旧 VN 独立 Host。Manager 使用 Slint，Family 以 in-process family plugin 运行；首轮提供 Windows 与 FVP。它不再作为 AstraEngine gameplay runtime provider，不依赖 RuntimeWorld、StateMachine、统一 package/save 或 Host VFS。

Family 持有原生 VM、文件访问、媒体解码、混音、最终画面和原生存档。Host 管理资料库、窗口、物理输入、音频设备、最终帧 HLSL 滤镜、插件与可选翻译服务。一个进程同时只运行一个 session。

保留现有资料库优先的 Slint 布局和元数据功能。新的资料库与设置 schema 明确重建旧数据；游戏原生存档独立于资料库。动态插件从本地显式安装，ABI/capability 校验失败即拒绝；多个 probe 命中由用户选择。

翻译 service 与配置页本轮实现，FVP 声明不支持正文替换，因此相关操作禁用。端到端翻译等待 Minori，不修改 RFVP 翻译路径。滤镜直接消费 Family 最终帧，提供缩放、锐化、Anime4K Restore_S/Upscale_S 和外部 Magpie format 4 HLSL 的明确兼容子集。

Artemis、KrKr、BGI、SoftPAL、Siglus、Minori 的研究与核心源码保留为后续接入材料，不构成本轮活动 Family 或运行时依赖。

## 设计与实施入口

- [共享 ABI 与 Host 契约](../contracts/astraemu-ipc.md)
- [实施方案与测试范围](../migrations/astraemu-independent-host.md)
- [当前状态](../status/stages/stage-5-astra-emu.md)
- [Family 研究索引](../emu/README.md)
- [Manager 使用手册](../manual/astraemu-manager-metadata.md)
