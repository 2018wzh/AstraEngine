# 路线图

## 1. 路线原则

AstraEngine 先完成可发布、可调试、可打包的原生 2D / VN-first runtime，
再扩展 AI 和 legacy compatibility。这里的 “UE-class runtime” 指工程完备度：
runtime 可脱离 Editor 完成 cook、package、launch、save、replay、debug、profile 和
release validation；不表示追求复杂 3D、FPS、高实时网络竞技、大型开放世界或完整
UE `UObject` / UHT / GC 体系。

优先级：

1. Foundation：Core + Platform + Module + Property。
2. Foundation：Actor/Component + EventBus + StateMachineRuntime。
3. Foundation：Asset + Media DTO + FilterGraph。
4. Foundation：ScriptRuntimeHost + Native VN / Lua。
5. Production runtime core：deterministic tick、save/replay、scheduler、diagnostics。
6. Production asset pipeline：import、cook、package、release gate。
7. Production media backend：real renderer/text/audio/timeline/filter execution。
8. Creator experience：template、Content Browser、Inspector、Graph/Timeline、PIE、Cook/Package。
9. Customization framework：plugin wizard、EngineModuleSlot、Editor panel、provider contracts、MCP tool。
10. Editor and runtime debugging。
11. AI MCP：Runtime AI MCP、Editor Copilot MCP、Editor Content Generation MCP。
12. Production hardening and UE-class 2D runtime acceptance。
13. Expansion track：legacy emulator / modernization plugins。

## 2. Completion Model

- `Foundation`：最小可运行基座，可通过 headless test、smoke program 或 demo 验证。
- `Feature Complete`：功能表面完整，覆盖真实项目主要工作流。
- `Production Ready`：具备真实后端、错误恢复、版本迁移、调试观测、压力测试和发布门禁。
- `UE-class 2D Runtime`：在 Astra 范围内达到可发布、可调试、可扩展、可维护的 runtime 完备度。

当前 Phase 1-4 属于 Foundation，不应被解读为 production complete。当前工作树另有 NativeVN runtime feature-complete evidence slice：动态链接的 `Astra*` engine DLL、NativeVN package manifest、headless run、media backend capability report 和 golden replay comparison 已实现，用于推进 README 的 runtime-first 验收链路；真实 binary asset cook、真实 image/font/audio execution backend、Editor、AI/MCP 和 Legacy 仍不在该 slice 内。

Phase 5+ 的实现入口以 production contract 文档为准：Runtime 见 `runtime-production-contract.md` 与 `save-replay-production-contract.md`，Asset/Package 见 `asset-package-production-contract.md`，Media/Decode 见 `media-backend-production-contract.md` 与 `hardware-media-decode.md`，Provider 见 `provider-contracts.md`，Editor/AI/Legacy/Release Gate 分别见对应 contract 文档。这些文档是准 API 草案，不代表当前生产代码已完成。

## 3. Phase 0：文档与工程基线

目标：

- 建立模块化 2D 引擎目标态和工程基线。
- 明确 Runtime 不依赖 Editor，VN、AI、Legacy 不污染 Core。

交付：

- 目标态设计文档和 ADR。
- 面向使用者的 `docs/manual` 手册骨架。
- Phase 0 文档检查脚本和 CI 门禁。
- 顶层 CMake / vcpkg / 目录结构。
- 编码规范。

验收：

- 新开发者能理解 AstraEngine 是模块化 2D 引擎，VN 是第一落地模块。
- 文档明确 Core 不绑定 VN、Live2D、AI 或旧 VM。
- `astra doc-check` 验证手册页面、文档链接、设计入口和过期 legacy 措辞。

非目标：

- 不实现完整 runtime 功能。
- 不实现 `astra` CLI、runtime sample、Editor 或 Foundation runtime target。

## 4. Phase 1：Foundation Core / Platform / Module / Property

状态：Implemented production Foundation gate slice. 当前实现覆盖 Core、Platform、ModuleRuntime、PropertySystem、示例动态模块、`astra --version`、`foundation_core_gate`、registered diagnostic-code release policy、release config hash、opaque dynamic library handle、service resolve audit、engine module slot policy validation、module release-gate binary hash、Property schema version/write policy 和 Catch2/CTest 验证；完整 platform input、hot reload、provider-specific production contracts 和 plugin wizard 仍是后续阶段。

目标：

- 建立可审计的引擎基础层、动态模块边界和 Foundation release-gate 证据。

交付：

- Core foundation、logging、error、config、time、path、diagnostic code registry、release policy 和 unknown-field migration policy。
- Platform window/input/filesystem/timer/thread、opaque dynamic library handle、file-watch polling 和 crash capture context。
- ModuleManager、ServiceRegistry resolve audit、ExtensionRegistry、EngineModuleRegistry policy validation、C ABI 和 module release-gate report。
- PropertySystem nested schema generation、schema version graph、write policy、diff/audit output 和 migration helpers。

验收：

- 示例模块可加载、注册服务、扩展和 provider，停用并卸载。
- ABI 不暴露 C++ ownership、Actor 指针或 native handles；public header isolation test 覆盖 SDL/OS/native handle 禁令。
- Headless backend 可执行基础 filesystem/timer/thread/crash smoke。
- PropertySystem 可生成 nested struct/array/map/tagged union JSON Schema、应用 defaults/validation、验证 schema version path 和执行 AI/editor/runtime/release 写入策略。
- `astra validate . --strict --json` 输出 `foundation_core_gate.passed = true`，并包含 registered diagnostic codes、release config hash 和 module binary SHA-256。

非目标：

- 不承诺完整 platform input、hot reload、provider-specific production contracts、plugin wizard 或 full runtime release gate。

## 5. Phase 2：Foundation Scene / Runtime

状态：Implemented foundation slice. 当前实现覆盖 headless `AstraScene` 和 Runtime module：`ActorWorld`、stable Actor/Component DTO、`ComponentDescriptor`、generation-safe handle、private EnTT-backed local storage、headless local ECS pack、`RuntimeWorld`、RuntimeEvent/EventBus、基础 StateMachine transition、Blackboard/ControlPolicy/Director foundation、foundation save/load、RuntimeReplay DTO 和 deterministic stable hash smoke。完整 lifecycle/prefab、production scheduler、timeline/resource/script/AI/module extension state 存档和 replay mismatch 定位仍属 Phase 5 及后续 production completion。

目标：

- 建立 headless actor/component/runtime 基座。

交付：

- ActorWorld、ActorId、ActorTypeId、ComponentDescriptor。
- EventBus、StateMachineRuntime、Blackboard、ControlPolicy、Director。
- Save/Load/Replay 基础快照。
- 局部 ECS system pack API。

验收：

- Headless world 可创建 Actor、派发事件、推进状态机、保存恢复。
- 存档不保存 native pointer 或 ECS entity 原始值。

非目标：

- 不承诺完整 lifecycle、prefab、deterministic scheduler、timeline/resource/script state 存档。
- EnTT 是 private implementation detail，不是 authoring model、ABI、save ID 或 Editor/MCP contract。

## 6. Phase 3：Foundation Asset / Media / FilterGraph

状态：Implemented foundation slice. 当前实现覆盖 `AstraAsset` 和 `AstraMedia`：asset URI/ID 解析、VFS mount、sidecar DTO/validation、registry scan、dependency diagnostics、import preset/project template/review item DTO、watch invalidation plumbing、PresentationCommand、RenderGraph/text/audio/filter DTO、FilterProfile validation/application、Renderer2D/TextLayout/Audio foundation provider descriptors、media release-gate foundation validation、mature backend capability probe（SDL3、libpng、libjpeg-turbo、libwebp、FreeType、HarfBuzz、miniaudio）、PNG/JPEG/WebP image metadata inspect API、image cook artifact metadata、HeadlessRenderer2D deterministic capture/hash，以及 SDL renderer factory private compile-path stub。真实 decoded texture upload、font atlas/shaped glyph execution、audio playback/mixing execution、GPU filter execution、cook/package binary transforms 和 package launch 仍属 Phase 6/7 production completion。

目标：

- 建立 asset sidecar、presentation DTO 和 headless media 验证链路。

交付：

- AssetId、VFS、AssetRegistry、sidecar。
- PresentationCommand、RenderGraph、Text/Audio command DTO。
- HeadlessRenderer2D、SDL minimal renderer factory。
- FilterGraph、FilterProfile、layer-aware targets。
- Hot reload watch plumbing。

验收：

- 可从 presentation event 生成背景、角色、文本、UI、音频和 filter command。
- FilterProfile 能应用到 background、character、ui、text、final 层。

非目标：

- 不实现真实 decoded texture upload、font atlas/glyph execution、audio playback、GPU filter execution、cook/package binary transforms；当前建立 mature backend capability evidence 和 image cook metadata evidence。

## 7. Phase 4：Foundation ScriptRuntimeHost / AstraVN

状态：Implemented foundation slice plus NativeVN runtime evidence. 当前实现覆盖 `AstraScript` 和 `AstraVN`：`ScriptRuntimeHost`、Native DSL parser、Lua provider via `sol2`、shared command stream、source diagnostics、debug-symbol DTO、`ScriptSnapshot`、`ScriptEventBridge`、VN event schema、预设 Actor/Component/StateMachine、`VnSession`、`VnSessionSnapshot`、Native/Lua parity headless hashes、NativeVN CLI smoke、save/restore evidence、package manifest evidence 和 golden replay comparison。完整脚本语言、debugger、hot reload、Graph/Timeline、真实 media backend 和 production release gate 仍属后续 completion。

目标：

- 建立脚本 runtime host、最小 VN DSL/Lua 和 playable VN demo。

交付：

- ScriptRuntimeHost、Astra Native Script、LuaRuntime、ScriptEventBridge。
- AstraVN DSL、VN Event、预定义 Actor 和状态机。
- 最小 VN demo 和 save/restore path。

验收：

- 脚本通过事件驱动 Actor 状态机。
- Demo 能显示背景/立绘/对白/选择的 presentation，记录音频/filter command，并保存恢复。

非目标：

- 不承诺完整脚本语言、debugger、sandbox、timeline graph 或真实 media 后端。

## 8. Phase 5：Runtime Core Completion

目标：

- 把 headless runtime 基座提升到真实项目可用的 deterministic runtime core。
- 实现入口：`runtime-production-contract.md` 和 `save-replay-production-contract.md`。

交付：

- Actor lifecycle、prefab/defaults、component schema migration。
- deterministic tick、task/coroutine scheduler、timer state、event ordering。
- ControlPolicy interrupt/queue/reject、priority inheritance、Director arbitration。
- complete save/load/replay：World、Scene、Actor、Component、StateMachine、Director、Timeline、resources、script state。
- memory/resource lifetime、error recovery、runtime diagnostics。

验收：

- 1000+ Actor、多状态机、脚本事件和 presentation event 可稳定 tick、保存、恢复和 replay。
- Runtime snapshot 有版本号、migration path 和 compatibility tests。
- Editor 只能通过公开 inspector/debugger 接口观察和控制 Runtime。

非目标：

- 不实现 Editor UI、不实现 legacy emulator。

## 9. Phase 6：Asset Pipeline Completion

目标：

- 建立可发布 runtime package 的内容管线。
- 实现入口：`asset-package-production-contract.md`。

交付：

- Importer、Cooker、DerivedDataCache、package manifest。
- asset dependency graph、incremental cook、hot reload invalidation。
- native/foreign/virtual mount policy。
- release gate：schema、broken dependency、license、AI provenance、package policy。

验收：

- native AstraVN 项目可从 source content cook 成 runtime package。
- Cooked package 可脱离 Editor 启动、加载资源、保存、恢复和 replay。
- Release gate 能阻止缺失资源、非法 foreign copy、未审核 AI asset 和不兼容 plugin 发布。

非目标：

- 不复制 legacy 外部资产；foreign asset 默认 mount-only。

## 10. Phase 7：Media Backend Completion

目标：

- 把 presentation DTO 提升为真实可执行的 2D media runtime。
- 实现入口：`hardware-media-decode.md` 和 `media-backend-production-contract.md`。

交付：

- real Renderer2D backend、texture decode/upload、sprite batching。
- text shaping、font atlas、fallback font、localization-friendly layout。
- audio mixer、streaming、voice/music/SFX routing。
- animation/timeline、camera、UI、effects。
- executable layer-aware FilterGraph。

验收：

- Runtime 可真实显示背景、角色、UI、文本和 filter output，可播放 voice/music/SFX。
- Headless backend 可验证 render order、filter target、text/audio command 和 deterministic output。
- Media backend 不向 public ABI 暴露 SDL、GPU handle、audio native handle 或 Editor widget。

非目标：

- 不实现复杂 3D renderer、physics renderer 或大型开放世界 streaming renderer。

## 11. Phase 8：Script And AstraVN Completion

目标：

- 完成原生 AstraVN 项目所需的脚本、VN graph 和运行时状态模型。

交付：

- stable Script API surface。
- Native DSL parser/runtime hardening。
- Lua runtime sandboxing and host API。
- VN graph/timeline/dialogue/choice/character/background/audio/camera integration。
- script debugger hooks and deterministic snapshot/restore。

验收：

- 完整 native AstraVN 项目可通过 DSL 或 Lua 运行同等故事流程。
- Dialogue、choice、character、background、audio、timeline、filter 和 camera 状态进入 save/replay。
- Script runtime 只能通过 Script API、RuntimeEvent 和 Presentation API 影响 world。

非目标：

- 不让 Lua、AI 或 legacy VM 成为 Core 依赖。

## 12. Phase 9：Creator Experience Rebuild

目标：

- 达到 UE 级创作者友好度，但限定在 2D / VN-first 范围。
- 实现入口：`editor-runtime-creator-contract.md`。

交付：

- Project Wizard、Template Browser、Content Browser、Asset Import Wizard。
- Actor/Component palette、Property Inspector、Prefab/Variant Browser。
- Script/Graph/Timeline/FilterGraph authoring workflow。
- Editor layout preset、command palette、asset picker、context menu、undo/redo、dirty/preview state。
- CreatorWorkflow sample：模板、导入/生成资产、Script/Graph/Timeline、PIE、Package。

验收：

- 新创作者可从模板创建项目、导入/生成资产、写对白和选择、PIE 调试并打包。
- 创作期修改有 undo/redo、diagnostics、review、preview 和 source/cooked 状态边界。
- Project Wizard、Content Browser、Inspector、Graph/Timeline 和 Package panel 都定义输入、输出、Runtime API、diagnostics、undo/redo、preview 和 save 行为。

非目标：

- 不让 Editor 成为 runtime 发布前置条件。

## 13. Phase 10：Customization Framework Rebuild

目标：

- 让插件作者和工具作者拥有 UE 级可定制入口，同时保持 Core 和 Runtime 边界稳定。
- 实现入口：`provider-contracts.md`。

交付：

- Plugin Wizard、descriptor schema、capability/permission 模板、sample plugin、diagnostics。
- Provider templates：Renderer2D、TextLayout、Audio、ScriptRuntime、PresentationLibrary、AssetImporter、CookProcessor。
- `IEditorPanelProvider`、`IMcpToolProvider`、`IAIProvider` 和 Editor command/menu/context action。
- EngineModuleSlot selection UI、project policy validation、packaged eligibility report。
- Plugin release gate、ABI compatibility tests、hot reload level policy。

验收：

- 插件作者可创建 Editor panel、asset importer、renderer/text/audio provider，并通过 release gate。
- 工具作者可添加 Editor panel 或 MCP tool，不修改 Runtime。
- Project policy 可替换 provider；错误选择产生可修复 diagnostics。
- 插件不能跨 ABI 暴露 C++ ownership、Editor widget 或 native handles。

非目标：

- 不允许插件替换 ModuleManager、Core diagnostics、PropertySystem 基础协议或 Runtime ownership。

## 14. Phase 11：Editor And Runtime Debugging

目标：

- 提供生产 runtime 所需的调试和 authoring 工具，但 Runtime 仍可独立发布。

交付：

- Project Browser、Asset Browser、Scene Tree。
- Script/Graph/Timeline/FilterGraph Editor。
- Actor、Component、StateMachine、Asset、Script Inspector。
- PIE、Runtime Debugger、Event Log、StateMachine visual debugger。
- Save/Replay inspector、asset dependency inspector、diagnostic panels。

验收：

- Editor 使用同一 Runtime，不走独立预览逻辑。
- 可查看和调试 Actor、Component、StateMachine、EventBus、Asset dependency 和 save/replay。
- Runtime 可脱离 Editor 发布。

非目标：

- Editor 不是 runtime 的前置条件。

## 15. Phase 12：AI MCP Collaboration And Runtime Safety

目标：

- 支持 Runtime AI MCP、Editor Copilot MCP 和 Editor Content Generation MCP，同时保持 deterministic save/replay。
- 实现入口：`ai-mcp-safety-contract.md`。

交付：

- Runtime MCP Host：feedback、context、intent request/validate/commit、fallback、audit。
- Editor Copilot MCP：suggestion、diagnostics explanation、patch proposal、test/cook/release gate assistance。
- Editor Content Generation MCP：draft generation、modification、enhancement、preview、review/import。
- Boundary Manager、Context Builder、AIEditRequest、AIPatchProposal。
- Provider permissions、SecretProvider、operation audit log。
- AI asset draft、sidecar import、generation provenance。
- Runtime AIIntent、IntentValidator、Director integration。

验收：

- Editor AI 建议进入 review queue 或 trusted write session。
- Editor Content Generation draft 必须 review 后才能进入 canonical source。
- Runtime AI 只能提交受控 Intent，并由 Director/ControlPolicy 仲裁。
- 已提交 AI 输出必须作为确定性数据进入 save/replay 和 release gate。

非目标：

- AI Provider 不进入 Core；AI 生成不绕过 review、audit 或 save determinism。

## 16. Phase 13：Production Hardening

目标：

- 让 runtime 具备长期发布、诊断、维护和兼容能力。
- 实现入口：`release-gate-observability-contract.md`。

交付：

- profiling/tracing、runtime markers、frame capture。
- crash/error reports、diagnostic channel、log routing。
- long-run soak tests、large-content stress tests。
- plugin ABI compatibility tests。
- save compatibility and migration tests。
- multi-config/multi-platform build matrix。
- release build packaging validation。

验收：

- Debug/Release 构建均可运行 test、smoke、package launch 和 release gate。
- 长时间运行、热重载、存档迁移、插件加载失败和资源缺失都有稳定 diagnostics。
- 性能、内存、asset load、render/audio/script tick 可被 profiling/tracing 捕获。

非目标：

- 不以 Editor 功能数量作为 runtime production readiness 的替代指标。

## 17. Phase 14：UE-class 2D Runtime Acceptance

目标：

- 在 Astra 的 2D / VN-first 范围内达到可发布 runtime 完备度。

交付：

- 完整 native AstraVN sample project。
- Source content -> cook -> package -> launch -> save -> replay -> debug -> profile -> release gate 全链路。
- Runtime 独立发布包。

验收：

- sample project 可在无 Editor 情况下运行完整流程。
- Runtime 支持真实 image/font/audio rendering、deterministic script execution、module/plugin loading、asset packages、development hot reload 和 release gate validation。
- Editor 支持模板创建、Content Browser、Graph/Timeline/Script 创作、PIE、Runtime Debugger、AI Review Queue 和打包发布。
- 插件可替换 renderer/text/audio 或添加 Editor panel / MCP tool，并通过 release gate。
- Runtime AI MCP、Editor Copilot MCP、Editor Content Generation MCP 的权限、审计、review、save/replay 策略通过验收。
- Core、Runtime、Asset、Media、Script、AstraVN 均有 unit、integration、headless、smoke、stress 和 compatibility tests。

非目标：

- 不要求复杂 3D、FPS、高实时网络竞技、大型开放世界或 UE UObject parity。

## 18. Expansion Track：Legacy VN Emulator / Modernization

目标：

- 在 native runtime production parity 之后，使用稳定 Runtime/Asset/Media/Script API 承载旧 VN 兼容和现代化。
- 实现入口：`legacy-compatibility-contract.md`。

交付：

- CompatRuntimeProvider、ForeignProjectProbe、PackageReader。
- Legacy VM state、opcode/timeline adapter、API Mapper。
- Save extension state。
- Compatibility Inspector。
- Modernization Profile、font replacement、UI overlay、FilterProfile。
- Mock legacy runtime fixture。
- BGI、Kirikiri、Ren'Py、NScripter prototype。
- Artemis prototype：unpacked-directory probe、`foreign-artemis:/` resolver、`.iet/.asb/.ast` index、minimal `e:*` host API、tag/API coverage report。

验收：

- 至少一个 mock legacy runtime 可运行并输出 VN presentation。
- Legacy save extension state 可进入 save/replay。
- Mount-only 默认不复制外部原始资产。
- Legacy 模块不反向污染 Core、Runtime、Asset、Media 的基础边界。
- Artemis prototype can map high-frequency tags to AstraVN Events without sharing Artemis VM control flow with AstraVN source languages。

非目标：

- Legacy 不是 native runtime 达到 UE-class 2D parity 的前置条件。
