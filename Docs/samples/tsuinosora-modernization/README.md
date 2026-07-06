# TsuiNoSora AstraVN Modernization Sample

本样例是 AstraVN 的真实项目压力规格，目标是把 1999 版《终之空》作为用户本地合法数据源，先转换为 AstraEngine Native 数据做完整验收，再规划通过 VFS 插件直读本地安装目录的发布形态。本目录只提交设计、研究、脱敏 report schema 和验收矩阵，不提交原始文件、解包文件、转换后商业 payload、截图、音频或影片。

## Sample Goal

Demo 需要同时证明两件事。第一，classic profile 可以保留原游戏的路线、文本、CG、音频、等待点、输入节奏、存读档和 replay 语义。第二，modern profile 可以在不破坏 classic profile 的前提下启用现代 VN 系统、视觉和音频修复增强、中文翻译补丁以及 Remake 版立绘 overlay。

这不是兼容器规格，也不要求第一阶段在运行时读取旧格式。主路线先把用户本地原版解包、重排并转换成 AstraEngine Native 数据，同时生成脱敏转换证据。后续阶段再发布补丁包、VFS 插件和 mount policy，由用户在本机提供原版安装目录。

## Profiles

| Profile | Purpose | Runtime shape | Acceptance focus |
| --- | --- | --- | --- |
| classic | 保留 1999 版内容和体验 | NativeVN 转换包，后续可由 VFS direct-read 替换来源 | 完整路线、媒体覆盖、输入节奏、save/load/replay、人工通关 signoff |
| modern | 在 classic 上叠加现代化能力 | profile package、filter preset、translation patch、portrait overlay | 标题、存读档、backlog、auto、skip、config、gallery、replay、route chart、voice replay、可回退增强 |

classic profile 是验收基线。modern profile 只能增加可关闭的 overlay、system page、filter/audio preset 和本地化包，不能改写剧情状态、backlog、read-state、save/replay 或 route 选择结果。

## Local Data Boundary

文档和 report 统一使用三个 root alias：

| Alias | Meaning | Repository rule |
| --- | --- | --- |
| `original_install_root` | 用户本地 1999 版安装目录 | 不提交路径、payload 或可复原内容 |
| `remake_install_root` | 用户本地 Remake 版安装目录 | 仅用于可选立绘 overlay 研究和本地转换 |
| `local_work_root` | 本地生成的 inventory、转换中间产物、NativeVN 包和 evidence | 只允许提交脱敏 schema、字段说明和汇总格式 |

允许提交的内容包括 logical id、资源类型、大小、hash、计数、覆盖率、诊断码和人工复核结果。不能提交商业文本全文、图像、音频、影片、解包后的文件、转换后的可游玩包或本地绝对路径。

## Reading Order

| Document | Use |
| --- | --- |
| [source-research.md](source-research.md) | 原版 Director/Shockwave 与 Remake PFS 资源形态、风险和不可提交内容 |
| [implementation-plan.md](implementation-plan.md) | 从本地解包转换到 VFS direct-read 的分阶段实施路线 |
| [conversion-manifest.md](conversion-manifest.md) | 脱敏 inventory、转换 report、modern profile report 和 manual signoff schema |
| [acceptance-matrix.md](acceptance-matrix.md) | classic/modern profile 的自动证据和人工验收标准 |
