# 总体架构

AstraEngine 系列采用“共享引擎核心 + 垂直产品 + 平台壳 + 扩展套件”的结构。AstraEngine 仓库维护公共契约，子仓只实现自己的产品面。

## 多仓职责

```text
AstraEngine
  core/runtime/asset-vfs/media/script/plugin/game-runtime/test contracts
AstraVN
  .astra canonical source, NativeVnRuntimeProvider, Luau policy, commercial VN baseline
AstraEditor
  Qt/QML creator editor, PIE, inspector, graph/timeline, release UI
AstraPlatform
  desktop/mobile/web/experimental native shells and platform decode
AstraEMU
  independent host, Slint manager, in-process family plugins, native files/save, async text service, final-frame HLSL filters
AstraRPG
  AstraRpgRuntimeProvider, RPG core, AI simulation, rpg.trpg ruleset/profile, local-private tabletop adapters, later Server/Client protocol
```

## 运行链路

```text
Text-first source (.astra/.yaml/assets)
  -> Import/Cook
  -> Asset VFS mount set
  -> Binary package
  -> GameRuntimeProvider
  -> RuntimeWorld
  -> Actor/Component + StateMachine
  -> PresentationCommand / AudioCommand / RuntimeEvent
  -> Renderer2D / TextLayout / AudioGraph / FilterGraph providers
  -> Save / Replay / ReleaseReport
```

RuntimeWorld 是组合 facade，不是全局单例。Editor、CLI、MCP、平台壳和测试框架都通过同一 public API 创建和驱动它。

## Target 与 Platform

Target 描述可执行产品形态：`Game` 用于可发布运行时，`Editor` 用于创作者工具，`Program` 用于 CLI、Manager 和离线工具。`Client`、`Server` 作为后续网络 stage 的 schema 保留值，不参与当前 release gate。

Platform 描述运行宿主能力：Windows、Linux、macOS、iOS、Android、Web 都通过 `PlatformCapabilityReport` 报告 renderer、decode、audio、filesystem、input、lifecycle、permission 和 SDK 状态。Package 同时携带 `target.manifest` 和 `platform.eligibility`；Release Gate 按 target、profile 和 platform report 判定。

## Core 边界

Core 包含基础类型、diagnostics、stable id、schema、migration、PropertySystem、ServiceRegistry、ExtensionRegistry、EngineModuleSlot 和插件加载策略。Core 不知道 VN、Editor、MCP、AI、Luau、legacy VM 或任何具体平台后端。

## Runtime 边界

Runtime 拥有 World、Scene、Actor、Component、StateMachine、EventBus、Scheduler、Director、ControlPolicy、Save/Replay 和 Debug API。Tokio task 可以服务 IO、decode、network 和工具任务，但 Runtime deterministic state 只在固定 tick 边界消费有序结果。

## Module Slot

可替换能力通过 EngineModuleSlot 和 ExtensionRegistry 明确选择，不按加载顺序抢占。默认 slot 包括 Renderer2D、TextLayout、AudioOutput、DecodeProvider、ScriptRuntime、PresentationLibrary、`vfs_provider`、ProductRuntimeProvider、AiProvider、TranslationProvider、MCPToolProvider。Runtime 不直接持有 AiProvider；运行时 AI 通过受限 MCP session 消费 typed Intent 和 committed output。

## 产品边界

AstraVN 是原生 VN 垂直模块，通过 `NativeVnRuntimeProvider` 接入 gameplay runtime。AstraEMU 使用同仓独立 Host 与 Slint Manager，不使用上述 RuntimeWorld/package 执行链。Family 自行读取原生文件、运行 VM、解码、混音、绘制和存档；Host 通过独立 Family ABI 消费最终帧与 PCM，并提供物理输入、窗口事件、可选异步翻译和 HLSL 滤镜。NativeVN 创作流程不依赖 EMU family。具体边界见 [ADR 0019](../adr/0019-astraemu-independent-host.md)。

AstraRPG 是后续 RPG 垂直模块，通过 `AstraRpgRuntimeProvider` 接入 gameplay runtime。它负责 map、party、inventory、quest、encounter、battle、AI agent intent、committed output 和 RPG-specific editor metadata。TRPG 玩法不作为独立产品模块；规则书适配、骰子、检定、ruling、seat authority 和 transcript 都落在 AstraRPG 的 `rpg.trpg` profile 里。CP2020 等规则书适配只能作为 local-private adapter，仓库只提交 schema、manifest、hash、coverage 和 diagnostic。

## v1 验收边界

全系列 v1 同时要求 EngineCore deterministic gate、NativeVN commercial baseline、UE 级 Editor workflow、六平台 profile gate、AI/MCP audit gate。AstraEMU Windows/FVP 使用独立产品测试范围。AstraRPG 是 Stage 7 planned extension，Server/Client protocol 是 Stage 8 planned extension；二者不阻塞当前 v1 gate。任一产品线可以独立开发，但 release 口径由本仓 contracts、implementation specs 和 status matrix 统一定义。
