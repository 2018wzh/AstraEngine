# Stage 5 AstraEMU

Status: `IN_PROGRESS`

2026-09-09：集成分支通过 Family ABI 7 项、Manager core 22 项和翻译服务 12 项测试，凭据相关联网测试按定义跳过；Headless 和 FVP 动态库构建通过。Manager 入口、原始字体选择及 Sandbox 游戏流程尚未完成。

2026 年 9 月 8 日，用户批准 [独立 Host 重构](../../migrations/astraemu-independent-host.md)。此前 RuntimeWorld/LegacyRuntimeProvider、Family ABI v9、Extension ABI、Layer2D/Hook/VFS 相关结果不代表新 Host 已实现。本轮按普通软件开发方式实施，不新增 evidence/report 留存体系。

## 当前工作

| 工作项 | 状态 | 完成条件 |
| --- | --- | --- |
| S5-FAMILY-01 独立 ABI | IN_PROGRESS | 静态/动态统一 lifecycle、借用帧、PCM、异步文本、错误与取消测试 |
| S5-FVP-01 RFVP 接入 | IN_PROGRESS | 自有 VM/文件/解码/混音/最终帧/原生存档，无旧 Host 依赖 |
| S5-MANAGER-01 独立 Host | IN_PROGRESS | 一个 session、真实输入/窗口事件、音视频、关闭返回资料库 |
| S5-MANAGER-UI-01 Slint | IN_PROGRESS | 复用现有资料库布局，设置与错误可操作，新数据 schema |
| S5-AUTOPROBE-01 插件 | IN_PROGRESS | 本地显式安装与 ABI/capability 校验，多项命中人工选择 |
| S5-METADATA-01 资料库 | IN_PROGRESS | 作品与安装记录、元数据和游玩记录接入新 core |
| S5-TEXT-01 异步翻译服务 | IN_PROGRESS | 配置、连接测试、取消、timeout、上下文与 session cache；FVP 禁用 |
| S5-FILTER-01 HLSL 效果链 | IN_PROGRESS | 缩放/锐化、Anime4K、外部效果、参数与原子配置切换 |
| S5-GATE-01 测试 | IN_PROGRESS | 增量回归、提交前检查和授权游戏完整路线 |
| S5-ARTEMIS-01 Artemis | PLANNED | 本轮不接入 |
| S5-KRKR-01 KrKr | PLANNED | 本轮不接入 |
| S5-BGI-01 BGI | PLANNED | 本轮不接入 |
| S5-SOFTPAL-01 SoftPAL | PLANNED | 本轮不接入 |
| S5-SIGLUS-01 Siglus | PLANNED | 本轮不接入 |

旧 S5-GAME-RUNTIME-01、S5-EMUCORE-SM-01、S5-LEGACY-VFS-01、S5-SCRIPT-01 和旧 CLI/Headless 工作项被本次边界取代，不继续实现。Minori 核心源码保留，端到端翻译等待其新 ABI 接入。

## 验证状态

独立 Family ABI 的 7 个测试、Manager core 的 22 个测试和翻译服务的 12 个测试已通过，三个 crate 的严格 Clippy 检查也已通过。滤镜模块已在 GPU 上完成非均匀输入、奇数尺寸边缘和完整像素读回测试，并运行了固定版本的 Magpie 原始 Restore_S/Upscale_S 文件；外部效果的输出尺寸已按实际声明处理。Slint 合成和实际游戏行为仍在集成。

Host 音频执行器已合入持久化重采样、跨块缓冲、取消唤醒、设备错误检查和单一有界队列。包含实际源码与真实 Family ABI 的临时测试工程通过 8 项测试和严格 Clippy；这不代表 Manager 二进制或物理音频设备已经通过测试，完整入口仍待集成。

文档校验脚本已更新，235 份 Markdown 检查通过。移除 EMU 耦合后，`astra-release` 的 43 项测试通过；提交前完整检查与 Sandbox 游戏流程尚未完成。

T-S5-INDEPENDENT-HOST 覆盖启动、输入、音视频、系统页、原生存读档、退出冷启动和任一结局。测试不启用 FVP 翻译。

Report Schema: 本轮不新增运行报告 schema。Sample: 授权本地游戏只用于临时测试，不进入仓库。

提交前检查仍按根 AGENTS.md 执行；不自动合并或发布。
