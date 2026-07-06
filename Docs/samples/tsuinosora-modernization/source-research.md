# Source Research

本页记录本地合法数据的资源形态和研究边界。所有文件名只按产品内逻辑名称或相对形态描述，不记录本机路径，也不包含商业 payload。

## 1999 Source Shape

1999 版样本表现为 Director/Shockwave 项目结构。入口和数据目录包含 Director movie、protected movie、cast library、Xtras 和安装程序相关文件；本地观察到的主体资源形态包括 `.dxr`、`.dir`、`.cxt`、`.i64`、`.dll`、`.x32` 和可执行入口。转换工具需要把这些文件当作原始 evidence source，而不是把 Director VM 直接放进 EngineCore。

第一阶段的目标是抽取可验证的内容索引：

| Domain | Expected source | Native target |
| --- | --- | --- |
| Script and route | Director movie 和 cast/script 资源 | `.astra` command stream、route table、source map |
| Dialogue text | cast member、script literal、外部文本资源 | text table、backlog id、localization key |
| Background and CG | bitmap/cast/media member | Asset registry image item、presentation command |
| Voice and sound | cast/media member、外部 audio | AudioGraph clip、voice replay id、fence |
| Movie | Director movie 或外部 movie resource | Media clip、movie wait state、skip/replay metadata |
| System behavior | 左键继续、右键存档等旧交互 | modern input map、system story、save/load adapter |

已知输入模型较窄：原版主要依赖左键推进和右键存档。classic profile 要复现这个节奏；modern profile 再把标题、设置、backlog、auto、skip、gallery、route chart 和 replay 补成 AstraVN 商业基线系统。

## Remake Source Shape

Remake 版先作为可选立绘 overlay 来源，不默认替换背景、CG、UI 或影片。该样本表现为 Artemis/PFS 系列资源形态，包含主 archive、patch-like archive、备份 archive、字体和 loose movie 文件。Artemis 侧的 archive、patch chain、script、presentation 和 media 研究已经在以下页面维护：

| Reference | Use |
| --- | --- |
| [../../emu/artemis/source-inventory.md](../../emu/artemis/source-inventory.md) | Remake/Artemis source inventory 口径 |
| [../../emu/artemis/archive-format.md](../../emu/artemis/archive-format.md) | PFS/PF8/PF6 archive 解析事实 |
| [../../emu/artemis/pfs-patch-chain.md](../../emu/artemis/pfs-patch-chain.md) | patch chain 与覆盖顺序 |
| [../../emu/artemis/presentation-and-media.md](../../emu/artemis/presentation-and-media.md) | 立绘、背景、音频、影片等 presentation 线索 |

TsuiNoSora modern profile 只读取 Remake 立绘候选，建立 alias/replacement review 表，并把每个替换映射记录到 `tsuinosora.modern_profile_report.v1`。任何替换都必须可独立关闭，关闭后 classic profile 的 hash、route 和 save/replay 结果不应变化。

## Risks

| Risk | Impact | Required mitigation |
| --- | --- | --- |
| Director/Shockwave 资源可能含受保护 movie 或压缩 cast | 自动抽取不完整，脚本 source map 断裂 | inventory 记录 coverage status，转换 report 标出人工复核项 |
| 旧版 timing 与输入循环依赖 Director 行为 | 等待点、音频 fence、movie skip 可能偏移 | scenario 记录 command cursor 和 wait state，人工验收记录感知差异 |
| 文本和媒体 ID 可能由脚本动态拼接 | 简单资源枚举无法覆盖 route | 以 playthrough coverage、resource access log 和 missing-resource report 交叉验证 |
| Remake 立绘风格和比例不同 | overlay 可能破坏画面构图 | 使用 alias/replacement review，逐项记录裁切、anchor、scale 和 fallback |
| 中文翻译来源权限未确定 | 无法把译文直接提交 | 第一阶段只定义 patch 接口、覆盖规则和验收口径 |
| VFS direct-read 需要用户本地安装目录 | 发布包不能独立包含商业内容 | runtime 只提交插件、patch 和脱敏 manifest，启动时由用户选择本地数据根 |

## Repository Boundary

仓库内只能出现：

- 文档、schema 说明、scenario 规格和验收矩阵。
- 脱敏 inventory 和转换 report 的字段定义。
- 不可逆 hash、计数、coverage 百分比、诊断码和人工 signoff 结果。
- 空的目录约定、忽略规则和生成命令说明。

仓库内不能出现：

- 原始、解包、转换后的商业图像、音频、影片、脚本或文本全文。
- 能绕过商业保护、授权检查或访问控制的说明。
- 用户本机路径、用户名、安装盘符、私有环境变量或 provider secret。
