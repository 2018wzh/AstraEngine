# Artemis Implementation Checklist

状态只描述 AstraEMU Artemis family adapter 的实施准备度，不把文档观察写成已实现 runtime。

## Archive

| Status | 项目 | 验收 |
| --- | --- | --- |
| DONE | PF6/PF8 header/index 结构已从 `pfs-rs` 确认 | 能读 magic、index size、entry count、entry path/offset/size |
| DONE | PF8 key 和 XOR 规则已确认 | Synthetic PF8 entry 能 round-trip |
| DONE_WITH_CONCERNS | PFS patch chain 规则已确认 | 需要 synthetic resolver test 覆盖 `.000`、`.002`、`.500` |
| DONE_WITH_CONCERNS | folder pack lookup 已从官方文档确认 | 本地样本未覆盖，需要 fixture |
| BLOCKED | exe+pfs 合并特殊格式 | 需要公开 fixture 或用户授权样本，不能从商业 exe 推断 |

## Boot and Script

| Status | 项目 | 验收 |
| --- | --- | --- |
| DONE | `system.ini` boot keys 已确认 | 能选 `WINDOWS` 段并读取 `WIDTH/HEIGHT/CHARSET/BOOT` |
| DONE | `.iet` text tag 语法已确认 | 能输出 tag、text span、Lua block hash |
| DONE_WITH_CONCERNS | `.ast` table row 形态已观察 | 需要 parser test 覆盖 tag row、label、正文本地结构化 |
| DONE_WITH_CONCERNS | `.asb` magic 和 string table 已观察 | opcode/table layout 未完成，先做 metadata probe |
| DONE | `.lua` 共享环境和 `calllua` 入口已确认 | boot 能注册 Lua 函数并调用 |

## Lua and Tags

| Status | 项目 | 验收 |
| --- | --- | --- |
| DONE | `calllua` engine object 规则已确认 | Lua function 第一个参数为 `e` |
| DONE | `setTagFilter` 返回 0/1 规则已确认 | filter 可继续或跳过原 tag |
| DONE | `e:tag` 字符串参数规则已确认 | 非字符串参数输出 diagnostics |
| DONE_WITH_CONCERNS | `e:enqueueTag` 顺序和 jump 清队列 | 需要 fixture 验证 call/return/jump 混用 |
| BLOCKED | `setScriptStack` 精确恢复语义 | 需要更多公开资料或 synthetic reverse test |

## Presentation and Media

| Status | 项目 | 验收 |
| --- | --- | --- |
| DONE | `lyc/lyprop/trans` 基础语义已整理 | 能映射到 layer load/prop/transition command |
| DONE | `splay/seplay/voice` 基础语义已整理 | 能输出 BGM/SE/voice command 和 voice replay ref |
| DONE_WITH_CONCERNS | `.sli` loop label | 已观察 sample position，需覆盖 BGM A-B loop |
| DONE_WITH_CONCERNS | full-screen movie | 已观察 loose ASF/WMV；后端选择归 Manager/provider |
| BLOCKED | MJA video layer | 需要公开 fixture，不从商业 payload 提取 |

## Runtime Family Plugin

| Status | 项目 | 验收 |
| --- | --- | --- |
| DONE | engine-native family plugin 边界 | 只输出 provider effect、Runtime event 和 report，不暴露 VM 内存 |
| DONE | deterministic wait model | 所有 wait/transition/audio/video fence 使用 `AwaitToken` |
| DONE_WITH_CONCERNS | Lua state snapshot | 先保存白名单 state；复杂 closure/coroutine 需 gate 标记 |
| DONE_WITH_CONCERNS | ASB execution | metadata probe 先行，执行器等 fixture |
| DONE | report 本地结构化规则 | 不输出 payload、剧情正文、截图、音频、视频帧、本机绝对路径 |

## 实施顺序

1. `ArtemisProbe`：index-only PF6/PF8 probe、movie magic、`system.ini` summary。
2. `ArtemisResolver`：loose/root/patch/folder pack lookup 和 safe path。
3. `SystemIniLoader`：平台段、stage、charset、boot。
4. `.iet` parser：tag、Lua block、source span、变量表达式延后求值。
5. Lua host：`calllua`、`setTagFilter`、`e:tag`、`e:enqueueTag`。
6. Presentation/audio tag subset：`lyc`、`lyprop`、`trans`、`splay`、`seplay`、`voice`、`wait`。
7. `.ast` parser：command row 和本地结构化 text capture。
8. ASB metadata probe：magic、string table、unsupported execution diagnostics。

## 必跑检查

文档阶段：

```bash
python Tools/check_docs.py
```

实现阶段再加：

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
