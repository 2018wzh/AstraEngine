# KrKr Runtime Family Plugin Design

KrKr family 以 engine-native plugin 接入 AstraEMU。Plugin 注册 `LegacyRuntimeProvider`，session 持有 XP3 VirtualStorage、TJS VM、KAG conductor、插件 facade、media state 和 snapshot store。Manager 只做窗口、输入、配置、provider selection、报告和 `RuntimeWorld` bridge。

## Session Modules

| 模块 | 职责 |
| --- | --- |
| `KrkrProbe` | 扫描目录、识别 XP3、plugin、savedata、standalone TJS |
| `KrkrRuntimeProvider` | 实现 lifecycle facade，隔离每个 case session |
| `Xp3Reader` | 读取 index、segment metadata、Adler-32、storage name |
| `VirtualStorage` | 生成 layer order、处理覆盖、提供 storage lookup |
| `TjsRuntime` | 执行 source/bytecode TJS，持有 `System`、`Storages`、`Scripts` |
| `KagRuntime` | 执行 `.ks`、tag handler、wait/trigger、macro、call stack |
| `BinaryScenarioRuntime` | 识别并执行 `.ks.scn`/PSB，未完成时输出 diagnostic |
| `PluginFacade` | 将旧 DLL/API 映射成 capability requirement 或受限 native requirement |
| `MediaBridge` | 输出 presentation/audio/movie command 和 media ref |
| `SnapshotStore` | 保存 legacy VM snapshot，返回 package/save snapshot section |

这是 public facade 之后的内部划分。不要再为每个 KrKr 插件建 public module；插件能力先统一进 `PluginFacade`。

## Lifecycle

`probe` 可以在不执行商业脚本的情况下完成 archive、plugin、media 和 script inventory。`open` 才初始化 VirtualStorage、TJS/KAG VM 和 PluginFacade。`step` 按 fixed tick 推进 KAG/TJS，遇到 wait、trigger、media fence、unsupported plugin 或 fault 时返回 `LegacyStepOutput`。

StateMachine 不理解 KAG tag、TJS object 或插件 API。session 只输出 stable trace、effect、AwaitToken 和 opaque snapshot section。

## Host Boundary

KrKr session 可以通过 host context 请求：

- image decode。
- audio decode。
- movie decode。
- font/text raster。
- file-like read-only storage。
- local report writer。

不能跨 ABI 传递 TJS object、KAG layer pointer、Actor pointer、GPU/audio native handle、Editor widget 或旧 DLL ownership。

## Plugin Policy

样本插件说明 KrKr case 会依赖大量 DLL。处理策略：

1. Probe 阶段记录 plugin name、hash、capability guess。
2. 如果能力已有 provider，用 provider 替代旧 DLL。
3. 如果必须旧 DLL，限制在 KrKr family plugin capability sandbox 内，并标记 native plugin requirement。
4. Manager 和 EngineCore 永不加载这些 DLL。
5. 网络、系统调用、shell、OLE 类插件默认 blocked，除非用户显式授权 case policy。

这能覆盖 `toml.dll`、`json.dll`、codec、movie、window/dialog 等常见能力，同时不把旧插件 API 扩散到新 runtime。

## Report

KrKr report 是 machine-readable，不包含商业 payload。必需字段：

```text
{
  family: "krkr",
  archives: [...],
  layers: [...],
  plugins: [...],
  scripts: [...],
  media_capabilities: [...],
  diagnostics: [...],
  boot_trace_hash,
  payload_policy: "metadata-only"
}
```

`boot_trace_hash` 用于 release gate 比对，不需要把台词、截图或音频样本提交到仓库。
