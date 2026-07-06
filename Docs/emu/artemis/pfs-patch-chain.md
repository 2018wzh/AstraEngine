# Artemis PFS Patch Chain

## 根包选择

官方 pack 文档给出的 Windows 根包顺序：

1. 与 exe 同名的 `.pfs`，例如 `Game.exe` 对应 `Game.pfs`。
2. `root.pfs`。
3. 特殊的 exe+pfs 合并形态。

AstraEMU 不需要执行 exe，也不依赖 exe 内部状态。Manager 在 case profile 中传入 data root 和可选 product executable name；family plugin 只用这些信息推导候选根包名。

## 文件查找顺序

官方文档描述的资源查找顺序是：

1. loose file。
2. 根包内的文件。
3. 当路径包含目录时，查找对应目录下的非根包。

示例：

```text
<game-root>/Game.pfs
<game-root>/image.pfs
<game-root>/sound.pfs
<game-root>/chara/stand.pfs

image/bg.png          -> image.pfs 中的 bg.png
sound/bgm/theme.ogg   -> sound.pfs 中 bgm/theme.ogg
chara/stand/a.png     -> chara/stand.pfs 中 a.png
```

性能上优先只使用根包，但 resolver 必须支持目录包，因为旧内容可能依赖这个规则。

## Patch 文件

官方 patch 文件名是根包名后加三位数字：

```text
root.pfs
root.pfs.000
root.pfs.001
...
root.pfs.999
```

patch 只需要包含被修复的文件。读取时，同一路径应命中编号更高的 patch，再落到编号更低的 patch，最后落到根包。AstraEMU 的 resolver 构建顺序可以从低到高扫描、后写覆盖，也可以从高到低查找、首个命中返回；对外结果必须一致。

## Observed Cases

| 样本 | 文件 | 结论 |
| --- | --- | --- |
| サクラノ詩10th | `sakuranouta10th.pfs`、`.000`、`.001`、`.002`、`.003`、`.500` | `.500` 仍符合官方三位 patch 规则，优先级高于 `.003` |
| 终之空Remake2025 | `tsuinosora_remake2025ver.pfs`、`.000` | 常规 PF8 root + PF8 patch |
| 终之空Remake2025 | `tsuinosora_remake2025ver.pfs.721.bak` | 内部是 PF6，但后缀不是官方 runtime patch 名；默认只作为 probe 可见文件，不进入 resolver |

## Resolver 设计

`ArtemisResolver` 维护三层映射：

| 层 | 输入 | 规则 |
| --- | --- | --- |
| loose layer | data root 文件树 | 最高优先级；只读；拒绝越界路径 |
| root chain | root pack + `.000` 到 `.999` | 同一路径高编号覆盖低编号 |
| folder packs | 路径首段对应的 `<folder>.pfs` 或嵌套 `<folder>/<name>.pfs` | 只在请求路径含目录时尝试 |

每个命中返回：

```text
source_kind = loose | root_pack | root_patch | folder_pack | folder_patch
source_name = pack file name or loose relative path
entry_name = normalized archive path
format = pf6 | pf8 | loose
size = payload size
encrypted = true | false
```

diagnostics 必须记录重复路径、patch 覆盖链、损坏包和被忽略的非标准后缀。报告里不能包含 payload bytes。

## Release Gate

Release gate baseline：

- 能识别 PF6/PF8 magic、index size、entry count、越界 entry。
- 能对两个样本生成同样的 root/patch 优先级摘要。
- 能说明 `.pfs.721.bak` 这类 backup 文件被发现但不进入默认 lookup。
- 能对 synthetic fixture 验证 loose file 覆盖 root，`.002` 覆盖 `.001` 和 `.000`。
