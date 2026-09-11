# Stage 5 AstraEMU

2026 年 9 月 11 日：RFVP 升级到上游 0.6.0（304e773387a9920c9db091ec1fd937c717aea949），保留 session-owned hosted 适配。FVP 的 26 项测试、RFVP hosted 的 43 项测试通过，Manager 编译通过。上游 TextWait、InputFlash 和 dissolve wait 修复已合入；本地另修复协程退出后被迟到的文本完成事件重新启动的问题。全局持久化已接入：启动在首个 tick 前加载，正常关闭通过原子替换写入；文件损坏、变量数量不符与 I/O 失败均返回错误。删除无调用方的 hosted snapshot/hash 与自定义 motion/graph 存档扩展，恢复上游普通存档布局。Sandbox 中修改两项游戏设置后完全退出并重启管理器，设置均保留。后续已通过当前集成工作区的全 workspace Clippy、Headless 构建和 workspace test；RFVP 内部适配单独提交为 bfc652cfc。游戏已检查标题、新游戏正文、普通槽位写入、重新创建 session 后读档及继续推进。载入会先恢复到存档页，退出该页后回到正文。Sandbox 采用显式 NullAudioDevice，真实声音、完整视频和结局仍待验证。

2026 年 9 月 10 日：独立 Manager 入口已集成，全 workspace 的格式、Clippy、Headless 构建与测试已通过。后续修正了 FVP 目录枚举和启动诊断，并加入显式选择的 NullAudioDevice 测试后端；FVP 的 18 项测试、Manager 的 26 项测试通过。真实设备缺失仍报错，NullAudioDevice 不输出声音。Sandbox 游戏流程、原生存档和视频测试仍在进行。

Status: `IN_PROGRESS`

2026-09-09：FVP 原始系统字体绑定已合入，集成分支的 16 项 FVP 测试通过，包含已安装 MS Gothic 的实际 face 解析、字体槽位和原生文件操作。RFVP 移至 `Emulator/ThirdParty/rfvp`，不再自动加入主 workspace；生产路径不再携带 Noto 替代字体。

2026-09-09：集成分支通过 Family ABI 7 项、Manager core 22 项和翻译服务 12 项测试，凭据相关联网测试按定义跳过；Headless 和 FVP 动态库已完成构建。Manager 入口及 Sandbox 游戏流程尚未完成。

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

Manager 作品详情已接入 VNDB/Bangumi 名称搜索与候选关联，移除设置页手填 ID。关联标题用于卡片、排序和本地搜索；解除关联后不读取旧快照。本轮 4 项元数据回归、Manager/UI Clippy、格式与文档检查通过；Windows Sandbox 已实际验证中文搜索、候选关联、空结果和重启保留，Bangumi 网络请求本轮未实测。`S5-METADATA-01` 保持 `IN_PROGRESS`。
