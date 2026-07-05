# BGI VM Dispatch

BGI VM dispatch 把 script opcode 转成 host capability 调用。AstraEMU BGI core 必须在 core 进程内消化这些调用，只向 Manager 输出稳定事件、trace、media block 和 snapshot。

## Dispatch group

BP bytecode 的 group opcode 后跟 call id。BCS 的 graph/sound/text command 也应归入同一类 domain，以便 replay 和诊断统一。

| Group | Domain | 说明 |
| ---: | --- | --- |
| `0x80` | `System` | program load、file query、wait、frame boundary。 |
| `0x81` | `System` | screen/config 扩展。 |
| `0x90` | `Graph` | resource load、node/object/surface。 |
| `0x91` | `Graph` | graph 扩展。 |
| `0x92` | `Graph` | text render 和 overlay。 |
| `0xA0` | `Sound` | BGM、SE、voice slot。 |
| `0xB0` | `User` | message/user extension。 |
| `0xC0` | `User` | user extension 2。 |

## 已知 host call

| Group:Call | 名称 | Arg count | 说明 |
| --- | --- | ---: | --- |
| `0x80:0x40` | `LoadProgram` | 2 | 加载 system/scenario program。 |
| `0x80:0x44` | `LoadProgramEx` | 5 | 带更多参数的 program load。 |
| `0x80:0x88` | `ScenarioCodePreprocess` | 0 | 对已加载 BCS 做 VM memory shadow。 |
| `0x80:0x18` | `WaitMilliseconds` | variable | 等待固定时间。 |
| `0x80:0x81` | `FrameBoundary` | 0 | 帧边界/yield。 |
| `0x81:0x60` | `ConfigureScreen` | variable | 配置画面。 |
| `0x81:0x64` | `ConfigureScreenSize` | 2 | 宽高。 |
| `0x90:0x10` | `GraphLoadResource` | 4 | 加载图像资源。 |
| `0x90:0x50` | `GraphCreateNode` | 0 | 创建 graph node。 |
| `0x90:0x56` | `GraphCreateObjectEx` | 7 | 创建对象，含资源字符串。 |
| `0x90:0x5C` | `GraphCreateObjectFull` | 17 | 完整对象创建。 |
| `0x90:0x60` | `GraphCreateObject` | variable | 创建对象。 |
| `0x90:0x80` | `GraphCreateSurface` | variable | 创建 surface。 |
| `0x90:0xBC` | `GraphPollObjectState` | variable | 查询对象状态。 |
| `0x90:0xBF` | `GraphPollObjectEvent` | variable | 查询对象事件。 |
| `0x92:0x9C` | `RenderText` | variable | 渲染文本。 |
| `0xA0:0x11` | `SoundPlayBgm` | 5 | 播放 BGM。 |
| `0xA0:0x20` | `SoundLoadSlot` | 3 | 加载 sound slot。 |
| `0xA0:0x24` | `SoundPlaySlot` | 3 | 播放 sound slot。 |
| `0xB0:0x80` | `UserMessage` | variable | 用户 message。 |
| `0xC0:0x00` | `User2SetScreenSize` | variable | screen size 扩展。 |

unknown call 不直接失败整个 core。默认策略是记录 `UnknownDispatch` diagnostic、保存 stack snapshot 和 source map；只有 release gate 明确要求时才把它升级为 hard error。

## 参数栈

VM host call 从 stack 取参。实现应把每次 dispatch 记录为：

```text
BgiDispatchTrace {
  source: BgiSourceSpan,
  domain: System | Graph | Sound | User,
  group: u8,
  call_id: u8,
  args: Vec<BgiValue>,
  return_value: Option<BgiValue>,
  diagnostics: Vec<BgiDiagnostic>,
}
```

字符串参数需要在 dispatch 前做 normalization：

- `System 0x80:0x34/0x35/0x40`：栈顶相关位置 0、1 可能是 string。
- `System 0x80:0x44`：位置 3、4 可能是 string。
- `Sound 0xA0:0x11`：位置 2、3 可能是 string。
- `Sound 0xA0:0x20`：位置 0、1 可能是 string。
- `Graph 0x90:0x56`：位置 0、3 可能是 string。
- `Graph 0x92:0x9C`：位置 0 可能是 string。

## Deterministic wait

Runtime 可异步加载 media 或等待输入，但 deterministic state 不能依赖 task completion order。任何 `WaitMilliseconds`、movie playback、voice completion、animation poll 或 user input 等待都必须落成可序列化 token：

```text
BgiAwaitToken {
  token_id: u64,
  kind: Time | Input | Animation | Audio | Movie | HostIo,
  source: BgiSourceSpan,
  requested_at_tick: u64,
  due_tick: Option<u64>,
}
```

token result 只在固定 VM tick 边界进入有序事件队列。save/replay 记录 token、result 和 dispatch trace，回放不重新请求外部 provider。
