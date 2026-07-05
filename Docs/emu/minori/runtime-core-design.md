# Minori Runtime Core Design

## 模块

```text
MinoriProbe
MinoriArchiveSet
PazReader
MinoriScriptDecoder
MinoriVm
MinoriPresentationMapper
MinoriAudioMapper
MinoriSnapshot
```

`PazReader` 只暴露 read-only bytes。`MinoriVm` 不直接打开文件；它通过 resolver 请求资源。

## IPC 输出

Core 输出：

- `RuntimeEvent::TextCapture`
- `PresentationCommand`
- `AudioCommand`
- `StateMachineTrace`
- `LegacyVmSnapshotRef`
- `Diagnostic`

## Error Policy

| 情况 | Diagnostic | 行为 |
| --- | --- | --- |
| 缺 PAZ key | `NeedsUserKey` | 阻止加载 case |
| entry 缺失 | `MissingResource` | 可恢复，输出 placeholder command |
| opcode 未识别 | `UnknownOpcode` | 保留 raw operand，继续或暂停由 severity 决定 |
| payload 解码失败 | `DecodeFailed` | 阻止使用该资源 |

## 最小可运行目标

第一版只要求 boot 到首个 message、推进文本、播放 BGM/voice/SE、显示背景和立绘、处理一次 choice、save/load 回到同一 pc。
