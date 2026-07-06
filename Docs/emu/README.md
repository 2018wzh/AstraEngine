# AstraEMU Legacy Engine Research

本目录保存 AstraEMU legacy family 的实现级研究资料。资料目标是服务 engine-native family plugin 编码：archive reader、脚本反编译、VM/tick、presentation/audio mapper、snapshot 和 release gate。架构边界以 [ADR 0012](../adr/0012-astraemu-engine-native-family-plugin.md) 和 [AstraEMU Family Plugin Contract](../contracts/astraemu-ipc.md) 为准。

## Engine 目录

Artemis 是 AstraEMU v1 的可用 family，其余 family 按 alpha probe profile 接入后再提升兼容深度。

| Engine | 合法样本来源 | 参考实现或工具 | 资料入口 |
| --- | --- | --- | --- |
| BGI / Ethornell | 用户本地合法安装和 synthetic fixture | 公开 Ethornell/BGI 研究与本仓 probe 工具 | [bgi/README.md](bgi/README.md) |
| FVP | 用户本地合法安装和 generated fixture | FVP 研究实现与本仓 HCB probe 工具 | [fvp/README.md](fvp/README.md) |
| Artemis | 用户本地合法安装和 synthetic PFS fixture | PFS/PF6/PF8 研究实现与 Artemis engine docs | [artemis/README.md](artemis/README.md) |
| Siglus | 公开体验版或用户本地合法安装 | Siglus 研究实现与本仓 Scene.pck probe 工具 | [siglus/README.md](siglus/README.md) |
| KrKr / KAG / TJS | 用户本地合法安装和 generated XP3 fixture | KiriKiri/KAG/TJS 公开研究资料 | [krkr/README.md](krkr/README.md) |
| SoftPAL | 用户本地合法安装和 synthetic PAC/DAT fixture | SoftPAL 研究实现与本仓 extcall probe 工具 | [softpal/README.md](softpal/README.md) |
| Minori | 用户本地合法安装和 synthetic PAZ fixture | Minori 研究实现与本仓 PAZ probe 工具 | [minori/README.md](minori/README.md) |

## 统一文档切分

每个 engine 至少包含：

```text
README.md
source-inventory.md
archive-format.md
script-format.md
script-execution.md
presentation-and-media.md
runtime-family-plugin.md
game-observations.md
tooling.md
implementation-checklist.md
```

特化文档只放 family 私有细节，例如 BGI 的 `script-bcs.md` / `script-bp.md`、Artemis 的 `script-tags-lua.md`、KrKr 的 `kag-tjs.md`、SoftPAL 的 `sv20-extcalls.md`。已存在的 `runtime-core-design.md` 继续保留文件名，但内容应按 ADR 0012 解释为 family plugin 内部设计。

## Tooling

Python 研究脚本位于 `Tools/AstraEMU/`。脚本默认执行 probe/list/decompile；extract 或写文件必须显式传 `--out`。Minori PAZ 和需要 key 的格式只接受外部 `--key-file` 或用户配置，源码不内置商业 key。
