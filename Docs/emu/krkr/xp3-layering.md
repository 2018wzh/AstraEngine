# XP3 Layering

KrKr 游戏常用多个 XP3 组成一个虚拟 storage。AstraEMU KrKr core 要把 archive mount 成有序 layer，后面的 layer 可以覆盖前面的同名 storage。覆盖结果必须可诊断，不能只返回最后一个文件。

## Layer Model

```text
VirtualStorage {
  key -> [
    { layer_rank, archive, storage_name, size, adler32, info_flags },
    ...
  ]
}
```

读取 `key` 时默认取 `layer_rank` 最大的条目；trace/report 同时保留被覆盖列表。这样才能解释“为什么某个场景来自 patch archive，而不是 base archive”。

实际 mount order 由 boot 脚本、命令行、engine 默认规则和补丁约定共同决定。样本目录中，base archive 和 patch archive 都放在根目录，文件名已经给出明显分层：`data.xp3`/`scn.xp3` 等是 base，`patch.xp3`、`patch2.xp3`、`LLLpatch.xp3`、`patchAI_UI.xp3`、`patchAI.xp3`、`yuzu_0012t_ai (1).xp3` 是补丁或整合包。AstraEMU 不应把字母序当成唯一真相；probe report 要记录最终采用的 mount order。

## 样本覆盖事实

| Patch | 覆盖条目数 | 主要覆盖对象 |
| --- | ---: | --- |
| `patch.xp3` | 593 | `scn.xp3` 的 125 个 `.ks.scn`，`fgimage.xp3` 的 433 个立绘资源，少量 UI/voice |
| `patch2.xp3` | 26 | `uipsd.xp3` 的 `_jp__pack.tlg` UI 图 |
| `LLLpatch.xp3` | 101 | 立绘 PBD、title logo PBD、UI PBD/TOML |
| `patchAI_UI.xp3` | 1 | `uitexts.toml` |
| `patchAI.xp3` | 274 | `scn.xp3` 的全部 138 个 `.ks.scn`、多份 TJS、`appconfig.tjs` |
| `yuzu_0012t_ai (1).xp3` | 503 | 合并覆盖 `patchAI`、UI TLG 和 base scene |

例子：`バンド001_03月_プロローグ上（現状）.ks.scn` 同时存在于 `scn.xp3`、`patch.xp3`、`patchAI.xp3` 和 `yuzu_0012t_ai (1).xp3`。report 应显示每一层的大小、flag 和来源，而不是只说“已加载场景”。

## Standalone Patch Script

根目录还有 `patch.tjs`。它是 `TJS2100` bytecode，不是 XP3。resolver 需要把它归为 boot patch candidate：

- 记录文件名、大小、hash、bytecode magic。
- 不尝试把它当 UTF-16 source 打印。
- 不把 bytecode 常量池写进 report，避免泄露商业文本。

## Resolver 规则

1. 读取所有候选 archive 的 index。
2. 根据 boot/mount 配置生成 `layer_rank`。
3. 用 KrKr storage key 建虚拟表，保留原始路径。
4. 对每个 key 标记 `active_source` 和 `shadowed_sources`。
5. 对无法解码的 archive、未知 index flag、未知 segment flag 输出 diagnostic。
6. 对补丁命中率输出统计，例如“patch 覆盖了 138/138 个 `.ks.scn`”。

这种做法比在读取时临时查多个 archive 更简单，也便于 release gate 复查补丁是否按预期生效。
