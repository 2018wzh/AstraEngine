# AstraEMU 核心资料

本目录保存归档、脚本、VM、媒体和原生存档的研究事实。运行边界以 [Family API](../../Emulator/Source/FamilyApi/astra-emu-family-api/API_CONTRACT.md) 和[重构契约](../contracts/rebuild.md)为准；当前问题与验证见[性能审计](../status/performance-upstream-audit-2026-10-12.md)。

## Engine 目录

当前整合 FVP、Siglus、Artemis、Musica 和 CMVS；KrKr 跳过本批适配，BGI/SoftPAL 保留研究资料。各核心实际完成范围见[实施计划](../status/implementation-plan.md)。

| Engine | 合法样本来源 | 参考实现或工具 | 资料入口 |
| --- | --- | --- | --- |
| BGI / Ethornell | 用户本地合法安装和 synthetic fixture | 公开 Ethornell/BGI 研究与本仓 probe 工具 | [bgi/README.md](bgi/README.md) |
| FVP | 用户本地合法安装和 generated fixture | FVP 研究实现与本仓 HCB probe 工具 | [fvp/README.md](fvp/README.md) |
| Artemis | 用户本地合法安装和 synthetic PFS fixture | PFS/PF6/PF8 研究实现与 Artemis engine docs | [artemis/README.md](artemis/README.md) |
| Siglus | 公开体验版或用户本地合法安装 | Siglus 研究实现与本仓 Scene.pck probe 工具 | [siglus/README.md](siglus/README.md) |
| KrKr / KAG / TJS | 用户本地合法安装和 generated XP3 fixture | KiriKiri/KAG/TJS 公开研究资料 | [krkr/README.md](krkr/README.md) |
| SoftPAL | 用户本地合法安装和 synthetic PAC/DAT fixture | SoftPAL 研究实现与本仓 extcall probe 工具 | [softpal/README.md](softpal/README.md) |
| Musica | 用户本地合法安装和 synthetic PAZ fixture | Musica 研究实现与本仓 PAZ probe 工具 | [musica/README.md](musica/README.md) |

## 文档职责

格式、源码索引、样本 metadata 和工具用法按核心保留。活动外部核心的精确基线与差异集中在各 Family 的 `MODIFICATIONS.md`，问题和下一步集中在 status，不再为每个核心维护第二份进度清单。

尚未接入核心的研究页不构成实现承诺；其中旧 RuntimeWorld、LegacyRuntimeProvider、snapshot bridge 或 release report 术语不能恢复为现行契约。

## Tooling

Python 研究脚本位于 `Tools/AstraEMU/`。脚本默认执行 probe/list/decompile；extract 或写文件必须显式传 `--out`。Musica PAZ 和需要 key 的格式只接受外部 `--key-file` 或用户配置，源码不内置商业 key。
