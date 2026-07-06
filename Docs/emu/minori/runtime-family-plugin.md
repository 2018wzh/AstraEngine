# Minori Runtime Family Plugin Design

Minori family 通过 `LegacyRuntimeProvider` 接入 AstraEMU。Session 持有 PAZ archive set、`.sc` decoder、VM、presentation/audio mapper、external key diagnostics 和 snapshot state。文档只描述 planned target，不表示 runtime 已实现。

## Session Modules

```text
MinoriRuntimeProvider
MinoriProbe
MinoriArchiveSet
PazReader
MinoriScriptDecoder
MinoriVm
MinoriPresentationMapper
MinoriAudioMapper
MinoriSnapshot
```

`PazReader` 只暴露 read-only bytes。`MinoriVm` 不直接打开文件；它通过 resolver 请求资源。PAZ key、exe patch、安装器保护和 hook 资料不进入公共实现。

## Lifecycle

`probe` 识别 PAZ、MYS、patch package、`.sc` script 和 key requirement。缺少外部 key 时只输出 diagnostic，不尝试提取材料。`open` 建立 archive set、script decoder、VM、presentation/audio state。`step` 推进 `.sc` 指令流，直到 message wait、choice、media wait、unsupported opcode、fault 或 halt。

## Step Output

Session 输出：

- `TextCaptureEvent`：message hash、speaker hash、line/source ref。
- `PresentationCommand`：背景、立绘、窗口和 transition。
- `AudioCommand`：BGM、voice、SE、movie ref。
- `StateMachineTrace`：script id、pc、opcode、wait reason。
- `Diagnostic`：missing key、missing resource、unknown opcode、decode failed。
- `LegacySnapshotEnvelope`：VM pc、stack、variables、choice state、presentation/audio state 和 resolver fingerprint。

## Error Policy

| 情况 | Diagnostic | 行为 |
| --- | --- | --- |
| 缺 PAZ key | `NeedsUserKey` | 阻止加载 case |
| entry 缺失 | `MissingResource` | 可恢复，输出 fallback command |
| opcode 未识别 | `UnknownOpcode` | 保留 raw operand，继续或暂停由 severity 决定 |
| payload 解码失败 | `DecodeFailed` | 阻止使用该资源 |

## First Implementation Gate

第一版只要求 boot 到首个 message、推进文本、播放 BGM/voice/SE、显示背景和立绘、处理一次 choice、save/load 回到同一 pc。
