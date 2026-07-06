# Siglus Runtime Family Plugin Design

Siglus family 在 AstraEMU 中作为 engine-native family plugin 实现。Plugin 持有原 Siglus 状态机和 VM，Manager 只做 host、窗口、输入、报告和 RuntimeWorld bridge。

## Provider 边界

```text
AstraEMU Manager
  RuntimeWorld
  LegacyVfsProvider
  LegacyScriptProvider
  LegacyActionProvider
  LegacyMediaMapper
Siglus Family Plugin
  ScenePackage
  SceneVm
  ResourceResolver
  MediaDecoder adapters
  Save/Snapshot bridge
```

Manager 不加载 Siglus 脚本，也不解析 `.ss` 私有栈。Plugin 输出：

| 输出 | 内容 |
| --- | --- |
| `RuntimeEvent` | boot、scene change、wait begin/end、shutdown |
| `PresentationCommand` | sprite、text window、wipe、effect |
| `AudioCommand` | BGM、voice、SE、movie audio |
| `TextCaptureEvent` | text hash、length、speaker hash、read flag、line |
| `StateMachineTrace` | scene、line、pc、command、wait kind |
| `LegacyVmSnapshotRef` | plugin 私有 snapshot section ref |
| `Diagnostics` | unknown form/opcode、missing resource、decode failure |

## 启动流程

1. `ProbeContent` 检查 `Scene.pck`、`Gameexe.dat`/`Gameexe.chs`、常见资源目录。
2. `LoadCase` 建立 resolver，读取 `Gameexe.*` config，加载 `Scene.pck` header 和 scene name table。
3. Plugin 选择 boot scene。若 `Gameexe.*` 指定 title/config/save/load scene，按 config 进入；否则从 scene 0 或可识别 main scene 启动。
4. VM 执行到第一个 wait/presentation boundary。
5. RuntimeWorld 收集渲染和音频命令。

## Resolver

Resolver 输入资源名和类型，输出候选路径：

```text
name
name.ext
g00/name.g00
g00/name.g01
bgm/name.ogg
bgm/name.owp
koe/zNNNN.ovk entry
koe/subdir/name.nwa
mov/name.omv
mov/name.mpg
mov/name.wmv
dat/name.*
```

路径匹配大小写按 Windows 兼容口径处理，但 report 中保留实际文件名。

## Gameexe config 依赖

`Gameexe.*` 影响窗口、message window、save/load/config scene、BGM/CG/database table、font、thumbnail、音量和 UI 资源名。Core 应把 config 解析成只读 table，运行时通过 normalized key 查。

公共契约不暴露 `GameexeConfig` 内部 map，只暴露本地结构化 diagnostics：

```text
config_file_present=true
config_decode_status=ok|blocked
required_keys_missing=[...]
```

## Wait 驱动

所有挂起动作必须落到 core 内 wait state。Manager 输入只排队，不抢先执行 VM：

```text
Step(tick)
  poll media/input
  if wait completed:
    enqueue wait result
  run VM until next boundary
  emit ordered events
```

这保证 replay 不依赖 host thread 或 decoder callback 顺序。

## Save/load

Siglus save/load 不能只保存画面。Snapshot 至少包含：

| 域 | 内容 |
| --- | --- |
| VM | scene、pc、line、栈、call stack、scene stack、user props |
| Runtime | stage/object/message/window/input/wait/audio/movie 状态 |
| Resource | loaded resource refs、resolver version |
| Config | Gameexe decode hash、resource root fingerprint |

外部 report 只保留 snapshot hash、scene/line 和 compatibility flags。

## 不做的事

Siglus family plugin 不提供 patch 注入、key 提取、商业包导出、脚本文本全文导出或 DRM/访问控制规避能力。需要 title-specific decode 材料时，只消费用户已合法提供的配置。
