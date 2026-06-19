# AstraEngine 目标与非目标

状态：Target Architecture

## 1. 总目标

AstraEngine 的目标是在 2D / VN-first 范围内达到 UE 去掉 Editor 后同级别的 runtime 工程完备度，
并提供 UE 级创作者友好度和可定制度。这里的 “UE 级” 是成熟度对标，不是复制 UE 的 3D、网络、
物理、UObject、UHT 或 GC 体系。

目标态必须同时满足三类用户：

- 创作者：能从模板创建项目，导入或生成角色、背景、音频、字体和 filter，编写 Script/Graph/Timeline，
  使用 PIE 和 Runtime Debugger 调试，然后 Cook/Package/Release。
- 插件作者：能通过 Plugin Wizard、C ABI、EngineModuleSlot 和 provider contract 替换或扩展
  renderer、text layout、audio、script runtime、presentation library、asset importer、cook processor、
  editor panel、MCP tool 或 AI provider。
- Runtime 发布者：能在无 Editor 环境启动 packaged runtime，完成 save/load/replay、profiling、
  diagnostics、release validation 和 deterministic content execution。

## 2. 成功状态

成功状态以完整 native AstraVN sample project 通过 release gate 为证据：

```text
Template -> Project -> Content -> Script/Graph/Timeline -> PIE -> Cook -> Package -> Launch
Save -> Load -> Replay -> Debug -> Profile -> Release Gate
```

验收必须覆盖：

- Runtime 独立于 Editor。
- Asset pipeline 从 canonical source 生成 cooked package。
- Media backend 真实显示 image/font/text/filter，播放 voice/music/SFX。
- Script runtime deterministic，可 snapshot/restore/debug。
- Editor 支持 Content Browser、Inspector、Graph/Timeline、PIE、Runtime Debugger 和 Package panel。
- AI 严格拆成 Runtime AI MCP、Editor Copilot MCP、Editor Content Generation MCP。
- Provider、MCP tool、Editor panel 和 EngineModuleSlot 可通过插件扩展并被 release gate 验证。
- 旧 VN compatibility 只作为 native runtime parity 之后的独立 AstraEmu Toolkit。

## 3. 非目标

- 不追求复杂 3D renderer、FPS、高实时网络竞技或大型开放世界。
- 不复制 UE `UObject`、UHT、完整反射 GC 或跨 ABI C++ Actor 继承。
- 不让 Editor、AI Provider、MCP server、Lua、Live2D 或 Legacy VM 进入 Core 依赖。
- 不把旧 VN 项目默认导入为 Astra canonical source。
- 不允许 AI 或 MCP 绕过 Review Queue、trusted session、Audit、Save/Replay 和 Release Gate。


