# Script Execution

KrKr compat core 持有旧 VM 和 KAG 状态机。Manager 只看规范化事件、presentation command、audio command、TextCaptureEvent、snapshot reference 和 diagnostics。

## 执行单位

每次 `Step` 只推进到下一个稳定边界：

- 遇到文本等待。
- 遇到 choice/input。
- 遇到 timer/media/transit wait。
- scenario/storage 跳转。
- save/load snapshot。
- shutdown 或 fatal diagnostic。

内部可以连续执行多个 TJS/KAG tag，但输出必须按 tick 排序。异步媒体、timer、input 和插件回调都转换为 `AwaitToken`，在固定 tick 边界进入有序队列。

## KAG 到 Runtime Event

| KAG/TJS 行为 | Core 输出 |
| --- | --- |
| 文本进入 message layer | `TextCaptureEvent`、`PresentationCommand::Text` |
| 背景/立绘/layer 变更 | `PresentationCommand::LayerUpdate` |
| transition/wait | `AwaitToken` + transition command |
| BGM/SE/voice | `AudioCommand` |
| movie | `PresentationCommand::Movie` + `AudioCommand` |
| choice/menu | `RuntimeEvent::ChoiceOpen` |
| jump/call/return | `StateMachineTrace` |
| save/load | `LegacyVmSnapshotRef` |

文本事件只保存本地结构化结构、speaker slot、voice id、storage/line/source ref 和 hash；本仓不提交商业文本。

## `.ks` 执行

`.ks` source parser 需要保留：

- label table。
- macro 展开前后的 source map。
- tag attributes 的原始 span。
- text line、line break、page break。
- call stack 和 return point。

`jump`、`call`、`return` 不直接改 Manager 状态。它们先进入 KrKr core 的 scenario stack，再由 core 输出 `StateMachineTrace`。

## `.ks.scn` 执行

`.ks.scn` 先按 binary scenario 处理。第一阶段可以做到：

- 识别 `PSB\0`。
- 输出 storage、size、hash、layer source。
- 报告 `unsupported_binary_scenario`。
- 不阻塞其他 archive/media/plugin probe。

实现执行器后，需要把 PSB 节点映射到同一套 KAG action：text、tag、jump、choice、wait、media、label。不要为 `.ks.scn` 再发明一套 public event。

## Snapshot

KrKr snapshot 至少包含：

- 当前 storage、label/offset、line 或 binary node id。
- KAG call stack、macro stack、wait token。
- `tf`、`sf`、系统变量和作品变量。
- message/backlog 状态。
- active layer、transition、BGM、SE、voice、movie。
- virtual storage layer fingerprint。
- plugin capability state 的可序列化部分。

Snapshot 由 compat core 持有，Manager 只拿 `LegacyVmSnapshotRef`。Astra runtime save 可以引用该 ref，但不能解析旧 VM 内存。

## Error Model

错误分三类：

| 类型 | 行为 |
| --- | --- |
| recoverable diagnostic | 缺插件、未知媒体格式、缺可选 storage，继续 probe |
| script stop | tag handler 缺失、storage 缺失、binary scenario 不支持，停在稳定边界 |
| fatal | index 损坏、VM 初始化失败、snapshot 不兼容，终止 case |

每条错误都带 storage、source ref、archive layer 和 capability name。没有这些字段，后续无法判断是脚本问题、补丁顺序问题还是 provider 问题。
