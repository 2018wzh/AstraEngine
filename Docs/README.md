# AstraEngine 文档入口

AstraEngine 文档按“产品目标 -> 共享契约 -> 模块规格 -> 平台与验收 -> 手册”的顺序组织。本仓是 AstraEngine 系列的总规格入口，子仓实现必须回到这里对齐公共边界。

## 阅读路径

| 路径 | 内容 |
| --- | --- |
| [product/vision.md](product/vision.md) | 产品定位、用户、非目标和硬验收 |
| [product/architecture.md](product/architecture.md) | 顶层分层、多仓职责和运行链路 |
| [product/roadmap.md](product/roadmap.md) | stage gate 与版本路线 |
| [contracts/README.md](contracts/README.md) | Runtime、插件、数据、媒体、AI 和 EMU 公共契约 |
| [implementation/README.md](implementation/README.md) | workspace、Runtime、Target、Provider、Asset/VFS、Game Runtime、parser、Luau policy、package、Editor、AI/MCP、Platform、EmulatorCore、Release Gate 实现蓝图 |
| [modules/README.md](modules/README.md) | EngineCore、AstraVN、Editor、AstraEMU 等模块规格 |
| [platforms/README.md](platforms/README.md) | 桌面、移动、Web 和实验平台边界 |
| [status/coverage-matrix.md](status/coverage-matrix.md) | 设计、API、数据、测试、release gate 和手册覆盖矩阵 |
| [manual/README.md](manual/README.md) | Creator、插件作者、Runtime/Platform operator 三本手册入口 |
| [samples/README.md](samples/README.md) | AstraVN 样例和真实项目压力样例索引 |
| [samples/astra-vn-script/README.md](samples/astra-vn-script/README.md) | AstraVN 脚本机制/策略分离样例 |
| [samples/tsuinosora-modernization/README.md](samples/tsuinosora-modernization/README.md) | TsuiNoSora classic/modern 双 profile 现代化移植验收样例 |
| [references/README.md](references/README.md) | sena-rs、rfvp、siglus_rs、ethornell-rs 参考案例 |
| [emu/README.md](emu/README.md) | AstraEMU legacy engine 研究资料和实现清单 |
| [migrations/README.md](migrations/README.md) | 已实现代码向 VFS 与 gameplay runtime 设计对齐的分步迁移计划 |
| [adr/README.md](adr/README.md) | 架构决策记录 |

## 维护规则

设计页不记录当前实现状态；状态和缺口统一进入 `status/`。任何模块新增 public contract 时，必须同步更新 coverage matrix 和对应手册入口。
