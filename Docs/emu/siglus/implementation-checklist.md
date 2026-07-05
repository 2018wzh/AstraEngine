# Siglus Implementation Checklist

Status 只描述 AstraEMU Siglus family 的实现清单状态，不表示本仓已完成 runtime 代码。

| Status | 含义 |
| --- | --- |
| `DONE` | 文档事实足够，后续可直接实现或验收 |
| `DONE_WITH_CONCERNS` | 可实现，但需要降级、额外 fixture 或 provider |
| `BLOCKED` | 没有合法输入或关键格式事实不足，不能实现完整行为 |

## 读取与探测

| Status | 项目 | 最小验收 |
| --- | --- | --- |
| DONE | Siglus root probe | 识别 root 和 `StartData/GameData` 布局 |
| DONE | `Scene.pck` package header | 输出 header fields、scene count、include prop/cmd count |
| DONE | scene name table | 能列 scene name；report 中只保留名称或 hash |
| DONE_WITH_CONCERNS | protected scene chunk decode | 接受授权 material；缺失时明确 blocked |
| DONE | `Gameexe.*` header | 读取 `version`、`exe_angou_mode`、size |
| DONE_WITH_CONCERNS | `Gameexe.*` body decode | 多编码和 LZSS 口径明确；受授权 material 约束 |

## 脚本执行

| Status | 项目 | 最小验收 |
| --- | --- | --- |
| DONE | `.ss` header parse | 33 个 `i32` 字段，offset bounds check |
| DONE | string table | UTF-16LE，按 string index XOR，输出 text hash |
| DONE | label/z-label/cmd-label | 控制流跳转和 farcall 可定位 |
| DONE | `CD_*` operand decoder | 覆盖 `0x00` 到 `0x34` 已知 opcode |
| DONE | stack model | int stack、str stack、element points 分离 |
| DONE_WITH_CONCERNS | high-level decompile | 非运行必需；不作为首阶段目标 |
| BLOCKED | 未知 title-specific form 完整行为 | 需要样本 trace 和 fixture 逐步补齐 |

## Presentation

| Status | 项目 | 最小验收 |
| --- | --- | --- |
| DONE | G00 type 0/type 2 header and decode contract | 能加载背景和 UI/mask 基础资源 |
| DONE_WITH_CONCERNS | G00 type 1/type 3 | 参考实现存在，样本覆盖需补 fixture |
| DONE_WITH_CONCERNS | advanced wipe/effect | 可先降级为 alpha/mask，report 标 concern |
| DONE | text window event | speaker/text hash、length、read flag、line |
| DONE_WITH_CONCERNS | font fallback | 需要平台 font provider 和 Gameexe font list |

## Media

| Status | 项目 | 最小验收 |
| --- | --- | --- |
| DONE | Ogg/Vorbis | 裸 `.ogg` provider |
| DONE_WITH_CONCERNS | OWP | 通过授权 stream provider，文档不记录 transform |
| DONE | OVK | entry table 和 bounded stream |
| DONE | NWA | 44-byte header、PCM output contract |
| DONE | OMV | header、display size、embedded `OggS` offset |
| DONE_WITH_CONCERNS | MPEG/WMV | 依赖平台或 FFmpeg provider |

## Save/load 与 replay

| Status | 项目 | 最小验收 |
| --- | --- | --- |
| DONE | local save snapshot domain | VM、scene、stack、runtime、resource refs |
| DONE_WITH_CONCERNS | original save binary compatibility | 需要 fixture 验证 |
| DONE | replay determinism rule | wait result 固定 tick 入队 |
| DONE | report payload policy | 不输出 payload、截图、完整文本、私有 stream |

## Release Gate

| Status | 项目 | 最小验收 |
| --- | --- | --- |
| DONE | probe-only report | root、header、extension counts、decode status |
| DONE_WITH_CONCERNS | full-flow scenario | 需要自制或可公开 fixture |
| DONE | failure classification | `DONE`/`DONE_WITH_CONCERNS`/`BLOCKED` |
| DONE | no-bypass audit | docs 和 report 不含 key、导出 payload 或补丁注入步骤 |
