# AstraEngine

状态：NativeVN playable v1 evidence slice / Dynamic engine linking

AstraEngine 的目标是在 2D / VN-first 范围内构建一个高度可定制、可发布、可调试、可扩展的模块化引擎。视觉小说和互动叙事是第一落地场景，但 Core 面向更广的 2D 叙事、演出和轻量玩法，而不是单一 VN 播放器。

当前仓库包含目标架构文档、ADR、CMake/vcpkg 工程基线、编码约束，以及 Phase 1 Foundation、Phase 2 Foundation Scene / Runtime、Phase 3 Foundation Asset / Media / FilterGraph、Phase 4 Foundation ScriptRuntimeHost / AstraVN、Phase 5 runtime evidence blockers 和 Phase 6 production Asset Pipeline slice。当前实现还加入了 NativeVN playable evidence slice 和 TsuiNoSora local playable fixture：`astra import/cook/package/run/replay/inspect` 可围绕 `Samples/NativeVN` 与 `Samples/TsuiNoSora` 生成 binary `.astrapkg`、embedded package payload table、local DDC artifact evidence、DDC corruption recovery evidence、engine DLL evidence、Script/AstraVN evidence、playable VN UI/system/save/load evidence、package manifest hash/provider feature hash replay evidence、package-payload media decode/RGBA texture/glyph/audio evidence、package integrity diagnostics 和 golden replay comparison。多数高层系统仍处于规划或待实现状态；阅读和实现时应区分“目标态”和“当前已存在的代码”。

## 设计北极星

AstraEngine 追求的是 **UE-class 2D runtime 工程完备度**，不是复制 UE 的功能版图。这里的 UE-class 指在 Astra 的 2D/VN-first 范围内，runtime 能脱离 Editor 完成：

- `validate -> cook -> package -> launch`
- `save -> load -> replay`
- `debug -> profile -> diagnostics -> release gate`
- provider、插件、脚本、资产、媒体后端和 AI/MCP 的可审计扩展

最终成功状态以完整 `NativeVN` sample project 通过 release gate 为证据，而不是仅靠概念文档或局部 demo。

## 当前范围

当前可确认的仓库内容：

- `Engine/Runtime/Core`：Phase 1 Core production Foundation gate slice，包含基础类型、diagnostics、diagnostic code registry、release policy、`spdlog`-backed structured logging、config profile/hash、time、path、serialization、unknown-field migration policy、stable id 和 build info。
- `Engine/Runtime/Platform`：Phase 1 Platform abstraction，包含 headless backend、SDL-backed window backend 编译路径、filesystem、timer、thread、opaque dynamic library handle、file-watch polling 和 crash capture context 服务。
- `Engine/Runtime/ModuleRuntime`：Phase 1 module foundation，包含 plugin descriptor validation、dependency resolver、C ABI、`ModuleManager`、`ServiceRegistry` resolve audit、`ExtensionRegistry`、engine module provider registry、slot policy validation 和 module release-gate report。
- `Engine/Runtime/PropertySystem`：Phase 1 reflection-lite foundation，包含 type/property descriptors、flags、validation/defaults、nested JSON Schema generation、schema version graph、write policy、diff/audit output 和 migration helpers。
- `Engine/Runtime/Scene`：Phase 2 Scene foundation，包含 `ActorWorld`、stable Actor/Component DTO、generation-safe handle、基础 lifecycle、component JSON data、world snapshot 和 private EnTT-backed local storage。
- `Engine/Runtime/Runtime`：Phase 2 Runtime foundation，包含 `RuntimeWorld`、`RuntimeEvent`、queued/deferred `EventBus`、基础 StateMachine transition、Director state、runtime snapshot、foundation save/load 和 deterministic hash smoke。
- `Engine/Runtime/Asset`：Phase 6 production Asset Pipeline slice，包含 asset URI/ID 解析、VFS mount、sidecar DTO/validation、registry scan、dependency diagnostics、import preset/project template/review item DTO、watch invalidation plumbing、production importer/cooker contracts、built-in importers/cook processors、local DDC reuse/rebuild/corruption recovery、binary `.astrapkg` writer/reader、zstd payloads、random-access/chunked package reads、read-only mount policy、Asset Release Gate report 和 hot reload rollback DTO。
- `Engine/Runtime/Media`：Phase 3 Media foundation，包含 PresentationCommand、RenderGraph/text/audio/filter DTO、FilterProfile validation/application、Renderer2D/TextLayout/Audio foundation provider descriptors、mature backend capability probe（SDL3、libpng、libjpeg-turbo、libwebp、FreeType、HarfBuzz、miniaudio）、PNG/JPEG/WebP image metadata inspect API、image cook artifact metadata evidence、media release-gate foundation validation、HeadlessRenderer2D deterministic capture/hash 和 SDL renderer factory private compile-path stub。
- `Engine/Runtime/Script`：Phase 4 Script foundation，包含 `ScriptRuntimeHost`、Native DSL parser、Lua provider via `sol2`、IR/debug-symbol DTO、`ScriptSnapshot` 和 `ScriptEventBridge`。
- `Engine/Runtime/AstraVN`：Phase 4 AstraVN foundation，包含 VN event schema、预设 Actor/Component/StateMachine、`VnSession`、`VnSessionSnapshot`、Native/Lua parity 和 headless presentation/save-restore evidence。
- `Engine/Plugins/Examples/Phase1Example`：示例动态模块，验证 C ABI lifecycle、service/extension/provider 注册和卸载。
- `Engine/Programs/astra`：runtime evidence CLI，支持 `--version`、`doc-check`、`validate`、`import`、`cook`、`package`、`run --headless-smoke/--windowed-smoke`、`replay --compare` 和 `inspect`。当前输出 `foundation_core_gate`、module release gate binary hash、registered diagnostic-code gate、engine DLL SHA-256 evidence、Phase 6 Asset Pipeline evidence、Phase 7 media backend provider evidence、media backend capability report、libpng image decode/RGBA texture smoke、image/font/audio cook metadata、NativeVN/TsuiNoSora Phase 4 Script/AstraVN evidence、`playable_vn` UI/system/save/load/timeline/media execution evidence、SDL/headless `window_present` frame evidence、binary `astra.package.manifest.v1` `.astrapkg` package manifest、zstd package payload table、package mount/payload read smoke、local DDC artifacts、DDC reuse/rebuild/corruption recovery evidence、package/cook/payload hash integrity diagnostics、package manifest hash/provider feature hash save-replay evidence 和 replay mismatch report。Editor/AI release gate、full Script debugger/Graph 和 per-driver visual/audio diff 仍是后续 production completion。
- `Samples/NativeVN`、`Samples/RuntimeStress`、`Samples/PackageSmoke`、`Samples/TsuiNoSora`：foundation/evidence sample descriptors；`NativeVN` 当前提供 Native DSL/Lua playable v1 route，并包含生成的可再分发 PNG/OGG fixture、UI、语音、音乐、SE、FilterProfile 和脚本 sidecar source assets，用于 AssetRegistry、dependency graph、cook manifest、local DDC artifact execution、package manifest、embedded payload table、PackageReader random-access/chunked-read/mount evidence、package integrity、save/load 和 golden replay evidence；`TsuiNoSora` 是 local-test-only fixture，用复制的 Artemis PNG/OGG/font/UI/system 资源验证真实资源 registry/cook/package/run、系统菜单、backlog、config、save/load 和 inspect evidence，不作为可再分发样例；`PackageSmoke` 提供 Phase 3 foundation smoke。
- `Engine/Tests`：Catch2 tests，覆盖 Phase 1 Foundation、Phase 2 Scene/Runtime foundation、Phase 3 Asset/Media/FilterGraph foundation、Phase 4 Script/AstraVN foundation、public header isolation、示例模块加载、save/load 和 replay hash smoke。
- `docs/design`：目标架构、路线图、系统规格、覆盖矩阵和 TODO。
- `docs/manual`：面向使用者的手册骨架和 Phase 1-4 Foundation 手册页，标注当前已实现与计划中内容。
- `docs/adr`：关键架构决策记录。
- `Engine/Programs/astra` 的 `doc-check`：文档结构、链接、设计入口和过期措辞检查。
- `cmake`、`CMakeLists.txt`、`vcpkg.json`：工程基线。
- `AGENTS.md`、`docs/coding-style.md`：实现和协作约束。

Phase 0 是文档与工程基线，已建立设计文档、ADR、手册骨架、CMake/vcpkg 基线、CI 文档检查和协作约束。Phase 1 已加入 Foundation Core / Platform / Module / Property 的 production Foundation gate slice 和测试/CLI 证据。Phase 2 已加入 headless Scene / Runtime foundation。Phase 3 已加入 Asset / Media / FilterGraph foundation，并补齐 media provider contract / release-gate foundation evidence。Phase 4 已加入 ScriptRuntimeHost / Native DSL / Lua via `sol2` / AstraVN headless foundation evidence。Phase 5 已补齐当前 NativeVN package-only save/replay evidence 所需的 package manifest hash、provider feature hash 和 replay mismatch localization。Phase 6 已加入 production Asset Pipeline slice。README 仍不应声称真实 Media backend、完整 Script debugger/hot reload/Graph/Timeline、production AstraVN、Editor、AI/MCP、Legacy、完整 samples、完整 scheduler、完整 prefab 或 full Editor/AI release gate 已实现，除非它们重新出现在工作树并通过验证。

## 核心定位

- AstraEngine 是模块化 2D 引擎，不是 AAA 通用 3D 引擎。
- VN / AstraVN 是第一垂直模块，不进入 Core。
- Core 不绑定 VN、Lua、Live2D、AI Provider、旧 VN VM、Editor 或 renderer backend。
- Runtime 必须可独立于 Editor 发布；Editor 是 authoring/debugger 工具，不是 packaged runtime 依赖。
- Legacy VN 兼容和现代化是 native runtime production parity 之后的 expansion track。

适合的目标场景：

- 传统视觉小说、ADV、互动小说。
- 动态漫画、动态绘本、点击解谜、养成、轻 RPG、回合制和卡牌。
- AI 协作式创作与运行时受控内容生成。
- 旧 VN 引擎模拟器、兼容运行和现代化表现。

明确非目标：

- 不追求复杂 3D renderer、FPS、高实时网络竞技或大型开放世界。
- 不复制 UE `UObject`、UHT、完整反射 GC 或跨 ABI C++ Actor 继承体系。
- 不让 Editor、AI Provider、MCP server、Lua、Live2D 或 Legacy VM 进入 Core 依赖。
- 不把旧 VN 项目默认导入为 Astra canonical source。
- 不允许 AI 或 MCP 绕过 Review Queue、trusted session、Audit、Save/Replay 和 Release Gate。

## 架构不变量

实现时优先保护这些边界：

- `Core -> none`：Core 不依赖 Platform、Runtime、VN、AI、MCP、Editor、Compat、SDL、Lua、renderer 或 audio。
- `Runtime` 不依赖 Editor UI、Editor widget、MCP server implementation 或 AI provider implementation。
- `Media` public API 不暴露 SDL、GPU handle、audio native handle。
- `Editor` 不拥有 runtime state，只能通过 public inspector/debugger API 观察和命令 runtime。
- `Compat` 不得作为 native runtime parity 的前置依赖。
- 创作者 DSL 不直接调用渲染或音频底层 API，而是转成 RuntimeEvent 和 PresentationCommand。
- AI 不直接跳剧情、不直接改核心变量、不直接调用底层 API。
- 存档保存确定性快照，不保存 native pointer、ECS entity 原始值、renderer/audio native handle 或 Editor-only object。

跨 ABI 禁止传递：

- STL ownership。
- C++ Actor/Component 指针。
- renderer/audio native handle。
- SDL/GPU/native platform handle。
- Editor widget。
- 内部 ECS entity 或 registry。

## 目标分层

目标架构的顶层系统包括：

- `Core`：基础类型、diagnostics、logging、config、time、path、serialization、stable id、PropertySystem。
- `Platform`：window/input/filesystem/timer/thread/dynamic library/headless 与 SDL-backed backend。
- `Module`：ModuleManager、ServiceRegistry、ExtensionRegistry、EngineModuleSlot、C ABI、plugin descriptor。
- `Scene`：World、Scene、Actor、Component、Prefab、local ECS boundary。
- `Runtime`：EventBus、StateMachineRuntime、Scheduler、Blackboard、ControlPolicy、Director、Save/Replay。
- `Asset`：AssetId、VFS、AssetRegistry、Importer、Cooker、DDC、Package Manifest、Hot Reload。
- `Media`：Renderer2D、TextLayout、Audio、Timeline、FilterGraph、headless verification。
- `Script`：ScriptRuntimeHost、Astra Native Script、Lua、Graph/Timeline、Legacy VM provider boundary。
- `Presentation / AstraVN`：Dialogue、Choice、Character、Background、Audio cue、VN StateMachines。
- `AI / MCP`：Runtime AI MCP、Editor Copilot MCP、Editor Content Generation MCP、Provider、Audit。
- `Editor`：Project Wizard、Content Browser、Inspector、Graph/Timeline、PIE、Runtime Debugger、Package panel。
- `Compat`：Legacy package reader、VM、API Mapper、Modernization Profile、Compatibility Inspector。

## Runtime 模型

公开运行时模型是 `Actor / Component + StateMachineRuntime`：

- Actor 是创作者、脚本、Editor、MCP、Save/Replay 和 Compatibility 共同可引用的对象模型。
- Component 保存数据和行为边界，受 PropertySystem metadata、schema、migration 和 inspector 支撑。
- StateMachine 是叙事、演出、UI、交互和 Runtime AI Intent 的核心运行抽象。
- ECS 只用于局部性能热点，不作为 authoring model、存档 ID 或动态模块 ABI。

核心运行链路：

```text
Creator DSL / Graph / Legacy VM / AI Intent
  -> RuntimeEvent
  -> Actor-bound StateMachine
  -> PresentationCommand
  -> Scene / Media / Asset / Audio / FilterGraph
```

Director、ControlPolicy、Blackboard 和 EventBus 是冲突仲裁与确定性执行的关键：

- `EventBus` 分发 RuntimeEvent、VNEvent、PresentationEvent。
- `Blackboard` 保存角色、场景或系统上下文。
- `ControlPolicy` 判断控制权、优先级、打断、排队或拒绝。
- `Director` 管理全局叙事阶段、Timeline lock、choice lock 和 AI permission window。

## 模块与可定制性

项目级扩展默认使用动态模块。模块通过 `AstraModule` C ABI 进入，通过 `ServiceRegistry` 获取服务，通过 `ExtensionRegistry` 注册能力。

模块生命周期目标：

```text
Discover
  -> ValidateDescriptor
  -> ResolveDependencies
  -> CheckVersion
  -> CheckPermissions
  -> LoadBinary
  -> Initialize
  -> RegisterExtensions
  -> Activate
  -> Deactivate
  -> Shutdown
  -> Unload
```

所有模块能力必须声明 capability；文件、网络、AI、runtime packaging、MCP、外部挂载必须声明 permission。Release Gate 校验 descriptor schema、ABI version、权限、依赖闭包、packaged eligibility、binary hash 和模块策略。

重要 extension / provider contract：

- `IEditorPanelProvider`
- `IAssetImporter`
- `ICookProcessor`
- `IScriptRuntimeProvider`
- `IPresentationLibraryProvider`
- `IRenderer2DProvider`
- `ITextLayoutProvider`
- `IAudioProvider`
- `IMcpToolProvider`
- `IAIProvider`

`EngineModuleSlot` 是深度替换能力的选择层，例如 renderer、text layout、audio、script runtime、presentation library、asset resolver、compat runtime。项目通过显式 policy 选择 provider，不按 priority 或加载顺序隐式替换。

不可替换的核心：

- ModuleManager、C ABI、ServiceRegistry、ExtensionRegistry。
- PropertySystem 基础类型协议。
- Core diagnostics、logging、config、path、time。
- Platform lifecycle、thread scheduler、render device core。

## 内容与资产原则

项目源数据必须适合人类、AI、MCP、Git diff、Review Queue、Cook 和 Release Gate。

- Canonical source 使用 YAML + JSON Schema。
- 二进制资源使用 sidecar 描述语义、来源、授权、cook、review 和 AI provenance。
- 所有 source object 必须有稳定 `id`。
- 列表项使用稳定 ID，不靠数组位置表达语义。
- `Cooked`、`DerivedDataCache`、package manifest 不是人工或 AI 编辑源。
- 外部原游戏资产使用 `foreign-*` AssetId，不能伪装成 `native:/`。
- Mount-only 项目默认不复制外部原始资产；现代化替换必须是授权的 `native:/` 资产。

资产状态流：

```text
External File -> Source Asset -> Registered Asset -> Cooked Asset -> Packaged Asset
AI Draft -> Review -> Accepted -> Source Asset
Generated/Enhanced Draft -> Review -> Accepted -> Source Asset
Rejected Draft -> Audit Only
```

Release Gate 应阻止：

- 缺失或重复 AssetId。
- 缺失 sidecar。
- broken dependency。
- invalid license。
- 未审核 AI draft。
- illegal foreign copy。
- FilterProfile target 错误。
- 插件 descriptor、permission 或 packaged eligibility 不合法。

## AI / MCP 边界

AI 被拆成三套独立工作流：

- `Runtime AI MCP`：根据玩家反馈和 runtime context 生成受控内容。
- `Editor Copilot MCP`：为创作者提供 suggestion、diagnostics explanation 和 patch proposal。
- `Editor Content Generation MCP`：生成、修改和增强内容 draft，经 Review Queue 后进入 canonical source。

共同约束：

- AI 不直接调用 Renderer、Audio、Asset、Script native API。
- Runtime AI 只能输出结构化 `AIIntent`，经过 IntentValidator、Director 和 ControlPolicy 后才能 commit。
- Runtime AI committed output 必须进入 Save/Replay；replay 不重新请求 provider。
- Editor AI 正式写入必须经过 Review Queue 或显式 trusted session。
- Content Generation draft 被接受前不能进入 AssetRegistry 或 Cook。
- Deterministic Build 默认阻止 runtime AI provider，但允许已审核并进入 Content 的 AI 资产。

## Save / Replay 目标

存档不能只保存 label 和变量。生产级 Save/Replay 需要保存：

- World、Scene、Actor、Component。
- 所有 StateMachine 当前状态和延迟事件。
- Blackboard、ControlPolicy lock、Director 状态。
- ScriptRuntime state、Timeline state、FilterProfile state、resource overrides。
- AI committed output、generation audit reference、fallback choice。
- module extension state；Legacy 仅作为 expansion extension state。
- 随机种子和 replay log。

Replay 应比较 state hash、event hash 和 presentation hash，并能把 mismatch 定位到 frame、event、actor、component 或 script command。

## Legacy Compatibility

Legacy VN emulator / modernization 是 expansion track，排在 native runtime production parity 之后。

目标：

- 通过 CompatRuntimeProvider、PackageReader、Legacy VM、API Mapper、Modernization Profile 和 Compatibility Inspector 支持旧 VN 项目运行、诊断、调试和现代化。
- 旧 VM 可以保存 PC、栈、变量、调用栈、timeline cursor 等私有状态，但必须通过 Save extension state 进入统一存档。
- Legacy API Mapper 输出 RuntimeEvent 或 PresentationCommand，不直接调用 Renderer2D 或 Audio native handle。
- FilterGraph 支持 layer-aware 现代化：background、character、ui、text、final。

非目标：

- 不默认破解、解密或绕过商业保护。
- 不把外部项目导入为 Astra canonical source。
- 不允许 legacy VM 或 compat package policy 反向污染 Core、Runtime、Asset、Media。
- 不把 Artemis/Kirikiri/BGI 等 VM 控制流暴露成 AstraVN source language feature。

## 路线图摘要

阶段优先级：

1. Phase 0：文档与工程基线。
2. Phase 1：Foundation Core / Platform / Module / Property。当前已实现 production Foundation gate slice。
3. Phase 2：Foundation Scene / Runtime。当前已实现 headless ActorWorld、RuntimeWorld、event/state-machine/save-load-replay foundation。
4. Phase 3：Foundation Asset / Media / FilterGraph。当前已实现 foundation 基座。
5. Phase 4：Foundation ScriptRuntimeHost / AstraVN。当前已实现 foundation 基座。
6. Phase 5：Runtime Core Completion。
7. Phase 6：Asset Pipeline Completion。
8. Phase 7：Media Backend Completion。
9. Phase 8：Script And AstraVN Completion。
10. Phase 9：Creator Experience Rebuild。
11. Phase 10：Customization Framework Rebuild。
12. Phase 11：Editor And Runtime Debugging。
13. Phase 12：AI MCP Collaboration And Runtime Safety。
14. Phase 13：Production Hardening。
15. Phase 14：UE-class 2D Runtime Acceptance。
16. Expansion Track：Legacy VN Emulator / Modernization。s

Completion model：

- `Foundation`：最小可运行基座，可通过 headless test、smoke program 或 demo 验证。
- `Feature Complete`：功能表面完整，覆盖真实项目主要工作流。
- `Production Ready`：具备真实后端、错误恢复、版本迁移、调试观测、压力测试和发布门禁。
- `UE-class 2D Runtime`：在 Astra 范围内达到可发布、可调试、可扩展、可维护的 runtime 完备度。

## 样例与验收证据

目标样例不是演示摆设，而是 release gate、文档、CLI、Editor 和 Runtime 的共同验收载体。

计划中的 sample：

- `NativeVN`：完整 native AstraVN vertical slice 和最终 acceptance sample。
- `RuntimeStress`：1000+ Actor、多状态机、多事件、多资源加载和长时间 soak。
- `PackageSmoke`：证明 packaged runtime 无 Editor 依赖。
- `ScriptParity`：证明 Native DSL、Lua、Graph/Timeline 共享 Runtime semantics。
- `MediaBackend`：证明真实 renderer/text/audio/filter output 和 headless verification。
- `AIIntentSafety`：证明 Runtime AI MCP 安全、可保存、可回放。
- `CreatorWorkflow`：证明模板、导入/生成资产、Script/Graph/Timeline、PIE、Package。
- `CustomizationPlugin`：证明插件作者可添加 Editor panel、asset importer、provider 和 MCP tool。
- `CompatMockExpansion`：证明 legacy expansion 不污染 native runtime。

当前 NativeVN / TsuiNoSora playable v1 evidence slice 已能跑通，并且 `--windowed-smoke` 会在 SDL3 窗口中提交 libpng 解码后的真实背景/角色/UI image primitives，以及 HarfBuzz shaping + FreeType rasterization 生成的 speaker/dialogue glyph primitives。对 `.astrapkg` target，窗口纹理和字体优先从 embedded package payload 读取，并在 `window_texture_sources` / `window_glyph_sources` 中标记为 `package_payload`，同时保留 deterministic frame hash：

```text
astra validate Samples/NativeVN --strict --json
astra cook Samples/NativeVN --config Release --json
astra package Samples/NativeVN --profile deterministic --json
astra run build/Saved/Packages/NativeVN.astrapkg --headless-smoke --json
astra run build/Saved/Packages/NativeVN.astrapkg --windowed-smoke --scripted-input Samples/NativeVN/Input/golden.yaml --auto-close --json
astra replay build/Saved/Replays/NativeVNGolden.replay --compare --json
astra inspect build/Saved/Packages/NativeVN.astrapkg --json
astra run build/Saved/Packages/TsuiNoSora.astrapkg --windowed-smoke --scripted-input Samples/TsuiNoSora/Input/golden.yaml --auto-close --json
astra doc-check
ctest --test-dir build -C Release --output-on-failure
```

这些命令证明当前 binary `.astrapkg` package/replay/playable workflow，包括窗口创建、scripted input、UI/system 状态、Phase 7 media provider/decode/timeline/filter evidence、save/load、package manifest hash/provider feature hash 和 replay route hash。最终 UE-class acceptance 仍要求 Editor/creator workflow、AI/MCP 策略、per-driver visual/audio diff 和长期压力/迁移测试等目标全部达成。

## 历史取舍

从提交历史看，项目曾从更 VN-first 的早期原型转向当前的模块化 2D engine baseline。

保留的长期方向：

- 模块化 2D 引擎；VN 是第一垂直模块。
- 动态模块、C ABI、ServiceRegistry、ExtensionRegistry、EngineModuleSlot。
- Actor/Component + StateMachineRuntime 作为公开 runtime 模型。
- PropertySystem 作为 Inspector、schema、serialization、AI review 和 MCP field editing 的轻量基础。
- Text-first source、sidecar、Review Queue、Release Gate。
- Native runtime parity 之后再做 Legacy compatibility。

被舍弃或后移的方向：

- 不恢复旧主线目标 `VNRuntimeServices`、`Bootstrap`、`AstraGame`。
- 不以旧 VN-first runtime 命名作为架构中心。
- 不把源码级 CMake 插件作为默认 public plugin model；动态模块和 C ABI 是默认扩展边界。
- 不把公开 Bevy/EnTT ECS runtime 作为 authoring、Editor、MCP、Save/Replay 或动态模块 ABI 中心。
- 不让 compat runtime 或 legacy VM 取代 native runtime 的达标路线。
- Renderer2D、RHI、TextCore、AudioCore、VFS、AssetRegistry、MinimalVN 等 production/runtime 能力应按 roadmap 重建，而不是恢复旧原型。

## 构建基线

当前工程使用 CMake、C++23 和 vcpkg 依赖。基础命令：

```powershell
cmake -S . -B build
cmake --build build --config Debug
ctest --test-dir build -C Debug --output-on-failure
build\Bin\astra.exe doc-check
```

也可以通过 CMake 运行 Phase 0 文档检查：

```powershell
cmake --build build --target AstraDocCheck
```

当前已有动态链接的 `Astra*` runtime/tool DLL targets、Phase 1 foundation runtime targets、Phase 2 Scene/Runtime foundation targets、Phase 3 Asset/Media/FilterGraph foundation targets、Phase 4 Script/AstraVN foundation targets、Phase 6 Asset Pipeline targets、Phase 7 Media Backend evidence、structured logging、media provider release-gate evidence、mature media backend capability evidence、libpng image metadata/RGBA decode evidence、image/font/audio cook metadata evidence、text glyph-run/audio logical-state/filter/timeline evidence、Native/Lua parity evidence、NativeVN 与 TsuiNoSora playable source asset sidecars、AssetRegistry/dependency graph、local DDC artifact execution/corruption recovery、binary `.astrapkg` zstd payload table、PackageReader random-access/chunked-read/mount evidence、Asset Release Gate evidence、cook/package integrity/replay/playable evidence 和 Catch2/CTest 测试。命令成功能证明当前 Foundation Core / Platform / Module / Property / Scene / Runtime / Media / Script / AstraVN、Phase 6 Asset Pipeline、Phase 7 Media Backend evidence 与 playable evidence slice 的测试面通过，不能作为完整 trace export、crash bundle、Script debugger/hot reload/Graph、Editor、GPU shader execution、PCM sample decode、video frame decode 或完整 AI/creator release pipeline 已实现的前提；应以实际工作树和 CI 输出为准。当前继续复用成熟库和工具链：`yaml-cpp`、`nlohmann_json`、`spdlog`、OpenSSL SHA-256、zstd、SDL3、libpng、libjpeg-turbo、libwebp、FreeType、HarfBuzz、miniaudio、FFmpeg、Lua、`sol2`、Catch2 和 CTest，不为解析、JSON、日志、哈希、压缩、图像 metadata decode、字体/文本 shaping/audio 基础能力探测、Lua 绑定或测试自造轮子。

## 实现时的快速判断

做新功能前先问：

- 它属于 Core、Runtime、Asset、Media、Script、AstraVN、Editor、AI/MCP、Compat 中哪一层？
- 是否让 Core 依赖 VN、AI、Lua、Legacy、Editor、Renderer 或 SDL？
- 是否跨 ABI 暴露了 ownership、C++ 指针、native handle 或 Editor widget？
- 是否能通过 PropertySystem/schema/diagnostics 被 Editor、CLI、MCP 和 Release Gate 共同理解？
- 是否能保存、加载和回放？
- 是否绕过了 Review Queue、Audit、Release Gate 或 deterministic package policy？
- 是否把 expansion track 当成 native runtime parity 前置？
- 是否有 sample、test、headless verification 或 release gate evidence？

如果答案会破坏架构不变量，应暂停实现，回到设计文档或 ADR 做取舍。

## 设计文档入口

推荐阅读顺序：

1. [docs/manual/README.md](docs/manual/README.md)
2. [docs/design/README.md](docs/design/README.md)
3. [docs/design/goals.md](docs/design/goals.md)
4. [docs/design/architecture.md](docs/design/architecture.md)
5. [docs/design/implementation-coverage.md](docs/design/implementation-coverage.md)
6. [docs/design/roadmap.md](docs/design/roadmap.md)
7. [docs/design/TODO.md](docs/design/TODO.md)
8. [docs/adr](docs/adr)

核心专题：

- [Foundation Core / Platform / Property](docs/design/foundation-core-platform-property.md)
- [Actor / Component / ECS Hybrid](docs/design/actor-component-ecs-hybrid.md)
- [Runtime Core](docs/design/runtime-core.md)
- [Asset Pipeline](docs/design/asset-pipeline.md)
- [Media Runtime](docs/design/media-runtime.md)
- [Script And Presentation](docs/design/script-and-presentation.md)
- [Extension And Module System](docs/design/extension-and-module-system.md)
- [Content And Assets](docs/design/content-and-assets.md)
- [Editor And Pipeline](docs/design/editor-and-pipeline.md)
- [AI Collaboration](docs/design/ai-collaboration.md)
- [MCP Integration](docs/design/mcp-integration.md)
- [Tools / Release / Observability](docs/design/tools-release-observability.md)
- [Samples And Test Matrix](docs/design/samples-and-test-matrix.md)
- [Compatibility Layer](docs/design/compatibility-layer.md)

手册入口：

- [Manual Root](docs/manual/README.md)
- [Getting Started](docs/manual/getting-started/README.md)
- [Programming](docs/manual/programming/README.md)
- [Systems](docs/manual/systems/README.md)
- [API Reference](docs/manual/api/README.md)
- [Editor](docs/manual/editor/README.md)
- [Samples](docs/manual/samples/README.md)
- [Migration](docs/manual/migration/README.md)
- [Release Notes](docs/manual/release-notes/README.md)
- [Concepts](docs/manual/concepts/README.md)
