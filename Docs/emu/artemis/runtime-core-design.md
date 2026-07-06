# Artemis Runtime Family Plugin Design

## Boundary

Artemis family 以 in-process plugin/provider 接入 AstraEMU。Manager 创建 RuntimeWorld，启用 Artemis plugin，挂载 VFS，注册 legacy action provider，并从 RuntimeWorld 收集 trace、presentation、audio、TextCaptureEvent、snapshot section 和 diagnostics。

Artemis plugin 内部可以按 Artemis 的 tag/Lua/PFS 规则实现；对外只暴露 engine-native provider contract：

```text
LegacyVfsProvider
LegacyScriptProvider
LegacyActionProvider
LegacyMediaMapper
LegacySnapshotCodec
ReleaseCheckProvider
```

EngineCore 不依赖 Artemis Lua、PFS、ASB、Windows movie backend 或具体 renderer/audio handle。

## Modules

| 模块 | 职责 |
| --- | --- |
| `ArtemisProbe` | 识别 data root、PF6/PF8 包、`system.ini`、boot script、movie header |
| `ArtemisResolver` | loose/root/folder/patch lookup，PF6/PF8 streaming read |
| `SystemIniLoader` | 读取平台段、stage、charset、boot、save policy |
| `ScriptStore` | `.iet/.ast/.asb/.lua/.ipt/.tbl/.sli` 只读解析和 source map |
| `TagExecutor` | 内置 tag、macro、自定义 tag、Lua filter dispatch |
| `LuaHost` | Lua sandbox、engine object、deterministic host capability |
| `PresentationModel` | layer tree、message layer、transition fence |
| `AudioModel` | BGM、SE、voice、loop marker、fade |
| `SnapshotCodec` | save/load、自描述 section、Lua serializable state |
| `TraceEmitter` | 本地结构化 trace、coverage、diagnostics |

## State Machine

```text
Unloaded
  -> Probed
  -> Loaded(system.ini + resolver + boot script)
  -> Running
  -> Waiting(AwaitToken)
  -> Saving/Loading
  -> Shutdown
```

所有等待都进入 `Waiting(AwaitToken)`，结果在固定 tick 边界回到 ordered event queue。异步 IO、图像加载、音频解码和视频 probe 可以并发，但不能直接修改 deterministic state。

## Public Contract

Artemis family plugin 的 public contract 是 case profile + report，不是脚本编辑器。

```yaml
schema: astra.emu.case_profile.v1
family: artemis
data_root: user_authorized_case_root
executable_name: optional_basename
platform: windows
```

输出 report 包含：

- pack list、format、entry count、index hash prefix。
- `system.ini` 选中的平台段、stage、charset、boot basename。
- script inventory、tag coverage、unsupported tag list。
- media inventory、magic、resolver source。
- trace coverage 和 release gate 结果。

不包含 payload、完整脚本、截图、音频、视频帧或本机绝对路径。

## Lua Sandbox

Lua host 只暴露 `engine` object。默认禁用：

- 文件写入和任意文件读取。
- 网络、HTTP、browser open、shell execute、native call。
- clipboard、系统震动、purchase、platform account。

需要兼容原 tag 时，以 capability 形式显式开启，并把结果固化进 save/replay。未开启时输出 recoverable diagnostics。

## Snapshot

Snapshot 使用 AstraEngine 自描述二进制容器，section payload 走 serde/postcard。最小 section：

| Section | 内容 |
| --- | --- |
| `artemis.header` | family version、engine fingerprint、case hash |
| `artemis.script` | current script id、row index、call stack、queued tags |
| `artemis.vars` | normal、`g.`、`t.`、`s.` store |
| `artemis.lua` | serializable Lua state 或 blocked reason |
| `artemis.presentation` | layer tree、message layer、transition state |
| `artemis.audio` | BGM/SE/voice state、loop marker |
| `artemis.backlog` | 本地结构化 backlog metadata、voice replay ref |
| `artemis.resolver` | pack index hash、patch chain fingerprint |

如果 Lua state 不能完整序列化，save/load gate 必须给出 `DONE_WITH_CONCERNS`，不能伪装成已完成。

## Diagnostics

| Level | 例子 |
| --- | --- |
| error | PFS index 越界、boot script 缺失、ASB 不可识别且必须执行 |
| warning | backup PFS 被忽略、只读 `s.` 变量被写入、未知 tag 被 macro 捕获 |
| info | patch 覆盖、movie loose file、PF6 包只读 |

诊断只用 basename、entry path、hash prefix 和 tag 名。

## Implementation Order

1. Probe + PF6/PF8 resolver。
2. `system.ini` + boot `.iet` parser。
3. Tag trace executor，只输出 command。
4. Lua host allowlist + `calllua`/`setTagFilter`/`e:tag`。
5. Presentation/audio command 映射。
6. `.ast` command row parser。
7. ASB metadata probe，执行语义在 fixture 补齐后再启用。
