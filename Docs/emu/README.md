# AstraEMU Legacy Engine Research

本目录保存 AstraEMU legacy family core 的实现级研究资料。资料目标是服务 compat core 编码：archive reader、脚本反编译、VM/tick、presentation/audio mapper、snapshot 和 release gate。

## Engine 目录

| Engine | 样本 | 参考实现或工具 | 资料入口 |
| --- | --- | --- | --- |
| BGI / Ethornell | `E:/Games/樱之诗春之雪`, `E:/Games/サクラノ詩`, `E:/Games/素晴らしき日々15th` | `D:/Workspace/ethornell-rs`, `D:/Workspace/BGITool` | [bgi/README.md](bgi/README.md) |
| FVP | `E:/Games/樱花萌放` | `D:/Workspace/rfvp` | [fvp/README.md](fvp/README.md) |
| Artemis | `E:/Games/サクラノ詩10th`, `E:/Games/终之空Remake2025` | `D:/Workspace/pfs-rs`, Artemis engine docs | [artemis/README.md](artemis/README.md) |
| Siglus | `E:/Games/anemoi 体験版`, `E:/Games/Rewrite_PLUS` | `D:/Workspace/siglus_rs` | [siglus/README.md](siglus/README.md) |
| KrKr / KAG / TJS | `D:/Downloads/3lj` | `D:/Workspace/FuckGalEngine/Krkr` | [krkr/README.md](krkr/README.md) |
| SoftPAL | `E:/Games/SteamLibrary/steamapps/common/koikake` | `D:/Workspace/sena-rs` | [softpal/README.md](softpal/README.md) |
| Minori | `E:/Games/夏空的英仙座（夏空のペルセウス）` | `D:/Workspace/FuckGalEngine/Minori` | [minori/README.md](minori/README.md) |

## 统一文档切分

每个 engine 至少包含：

```text
README.md
source-inventory.md
archive-format.md
script-format.md
script-execution.md
presentation-and-media.md
runtime-core-design.md
game-observations.md
tooling.md
implementation-checklist.md
```

特化文档只放 family 私有细节，例如 BGI 的 `script-bcs.md` / `script-bp.md`、Artemis 的 `script-tags-lua.md`、KrKr 的 `kag-tjs.md`、SoftPAL 的 `sv20-extcalls.md`。

## Tooling

Python 研究脚本位于 `Tools/AstraEMU/`。脚本默认执行 probe/list/decompile；extract 或写文件必须显式传 `--out`。Minori PAZ 和需要 key 的格式只接受外部 `--key-file` 或用户配置，源码不内置商业 key。
