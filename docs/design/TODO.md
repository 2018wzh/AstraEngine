# TODO


## 1. UE-style Documentation System First

目标：开发与文档同步进行，形成类似 UE 文档的信息架构，避免实现先行后补文档。

- [ ] Docs：建立 `docs/manual`，作为面向引擎使用者的开发文档根目录。
- [ ] Docs：建立 `docs/manual/getting-started`：安装、构建、创建项目、运行 sample、打包。
- [ ] Docs：建立 `docs/manual/programming`：Core、Module、Actor/Component、RuntimeEvent、StateMachine、Asset、Media、Script、AstraVN。
- [ ] Docs：建立 `docs/manual/systems`：Asset Pipeline、Cook/Package、Save/Replay、Renderer2D、Text/Font、Audio、FilterGraph、Hot Reload、Diagnostics。
- [ ] Docs：建立 `docs/manual/api`：public headers 的稳定 API reference 索引。
- [ ] Docs：建立 `docs/manual/editor`：Editor、PIE、Inspector、Debugger、Review Queue。
- [ ] Docs：建立 `docs/manual/samples`：完整 native AstraVN sample 的逐步教程。
- [ ] Docs：建立 `docs/manual/migration`：snapshot/schema/plugin ABI 迁移指南。
- [ ] Docs：建立 `docs/manual/release-notes`：每个 milestone 的变更、破坏性改动、验证命令。
- [ ] Docs：建立 `docs/manual/concepts`：Astra 与 UE runtime parity 的对标边界和非目标。
- [ ] Docs：为文档增加链接检查、过期检查、代码示例编译或片段验证策略。
- [ ] Design：保持 `runtime-core.md`、`media-runtime.md`、`script-and-presentation.md` 与实现同步。
- [ ] Design：保持 `foundation-core-platform-property.md`、`asset-pipeline.md`、`tools-release-observability.md` 与实现同步。
- [ ] Design：保持 `samples-and-test-matrix.md` 和 `implementation-coverage.md` 与验收证据同步。
- [ ] Design：每个 public runtime/editor/tool contract 必须在 design doc、manual page、schema/test 中至少各有一个权威引用。

验收：

- 新模块 landing page 必须包含：Overview、Key Concepts、Architecture、Programming Guide、API Reference、Examples、Troubleshooting。
- 每个 milestone 合并前必须更新 manual、design、development、release notes。
- 不允许只有 TODO 或 ADR，没有面向开发者的操作文档。

## 2. Rebuild Foundation：Repository And Build System

目标：建立工程骨架，使生产 runtime、Editor、工具、测试和 sample 的边界从一开始清晰。

- [ ] Build：定义新顶层目标命名规则：`Astra_<Module>`、`Astra_<Tool>`、`Astra_<Sample>`、`Astra_<TestSuite>`。
- [ ] Build：拆分 runtime、editor、developer tools、samples、tests、plugins 的 CMake option。
- [ ] Build：建立 Debug、RelWithDebInfo、Release 三种默认配置和 artifact 输出规范。
- [ ] Build：建立 platform abstraction 层构建矩阵，首批至少 Windows + headless CI。
- [ ] Build：建立 third-party dependency policy：允许、封装边界、ABI 暴露禁令、版本锁定。
- [ ] Build：建立 generated/cooked/cache 目录约束，禁止源树污染。
- [ ] Build：建立 `AstraBuildInfo`：版本、git commit、build config、feature flags、ABI version。
- [ ] Build：建立 CLI 工具入口：`astra`，包含 validate、cook、package、run、inspect、doc-check。
- [ ] Build：建立 sample project 目录规范：`Samples/NativeVN`, `Samples/RuntimeStress`, `Samples/PackageSmoke`。
- [ ] Test：建立测试分类和命名：unit、integration、headless、smoke、stress、compat、release-gate。

验收：

- 空构建、只构建 runtime、只构建 tools、构建 tests、构建 samples 均可独立完成。
- 构建产物不要求 Editor 存在即可运行 runtime sample。
- `astra --version` 输出 build info 和 enabled module list。

## 3. Core Rebuild

目标：建立稳定基础层，支撑 UE-class runtime 所需的诊断、配置、序列化、版本迁移和文档化 API。

- [ ] Core：定义基础类型策略：固定宽度整数、UTF-8、path、span/string view、expected/result、error code。
- [ ] Core：重建 diagnostics：category、severity、code、source location、context object、machine-readable payload。
- [ ] Core：重建 logging：channel、sink、structured fields、runtime/editor/tool routing、file rotation。
- [ ] Core：重建 assert/error policy：developer assert、recoverable runtime error、fatal error、release behavior。
- [ ] Core：重建 config：project config、runtime config、platform overrides、module policy、release profile。
- [ ] Core：重建 time：monotonic clock、game time、real time、fixed step、pausable timers、serialized timer state。
- [ ] Core：重建 path/file utility：canonical project path、package path、user save path、cache path。
- [ ] Core：实现 serialization framework：versioned document、schema id、migration registry、unknown field policy。
- [ ] Core：实现 stable id framework：TypeId、PropertyId、AssetId、ActorId、ComponentId、EventTypeId。
- [ ] Core：实现 telemetry/profiling marker API，不依赖 Editor。
- [ ] Core：按 `docs/design/foundation-core-platform-property.md` 实现 diagnostics、config、serialization、stable id 和 PropertySystem contract。
- [ ] Docs：编写 Core Programming Guide、Diagnostics Guide、Serialization/Migration Guide。

验收：

- Core 无 SDL、Lua、AI、VN、Editor、renderer、audio 依赖。
- Diagnostics 可被 CLI、Runtime、Editor、Release Gate 统一消费。

## 4. Platform Rebuild

目标：提供 runtime 可发布所需的平台抽象，同时 public API 不泄漏 SDL 或 OS handle。

- [ ] Platform：定义 window service、monitor/DPI、clipboard、cursor、display mode。
- [ ] Platform：定义 input service：keyboard、mouse、text input、gamepad、touch extension point。
- [ ] Platform：定义 filesystem service：project mount、user save、cache、package read、watch。
- [ ] Platform：定义 dynamic library service：load、symbol、version check、safe unload policy。
- [ ] Platform：定义 thread service：worker pool、main thread dispatch、job tags、shutdown order。
- [ ] Platform：定义 timer service：high-resolution time、sleep/yield、frame pacing hooks。
- [ ] Platform：定义 crash/error hooks：minidump path、last log capture、fatal diagnostic packet。
- [ ] Platform：实现 headless backend，作为 CI 和 server-style runtime 验证环境。
- [ ] Platform：实现 SDL-backed backend，所有 SDL 类型限制在 private implementation。
- [ ] Platform：按 `docs/design/foundation-core-platform-property.md` 实现 headless/SDL backend、filesystem/thread/timer/crash service contract。
- [ ] Docs：编写 Platform Programming Guide 和 Backend Porting Guide。

验收：

- Headless sample 不创建窗口也能运行 runtime tick、save/replay、cook/package validation。
- SDL sample 可创建窗口、输入事件、退出事件，但 public headers 不包含 SDL 类型。
- 文件监听、动态库加载和线程池有错误恢复测试。

## 5. Module Runtime Rebuild

目标：建立类似 UE module/plugin 体系的运行时扩展能力，但以 C ABI 和显式权限为稳定边界。

- [ ] Module：实现 plugin descriptor schema：id、version、api range、modules、dependencies、capabilities、permissions、packaged eligibility。
- [ ] Module：按 `docs/design/extension-and-module-system.md` 实现 descriptor validation、C ABI lifetime、ServiceRegistry、ExtensionRegistry、EngineModuleSlot contract。
- [ ] Module：实现 descriptor validator，输出 machine-readable diagnostics。
- [ ] Module：实现 dependency resolver：version range、load phase、optional dependency、cycle diagnostics。
- [ ] Module：实现 C ABI：host API、module API、lifecycle、diagnostics、opaque handles。
- [ ] Module：实现 ServiceRegistry：service id、capability view、permission gate、lifetime owner、resolve audit。
- [ ] Module：实现 ExtensionRegistry：extension kind、descriptor payload、duplicate policy、query filter。
- [ ] Module：实现 EngineModuleSlot：slot、provider、default provider、project selection、release policy。
- [ ] Module：实现 module lifecycle：discover、validate、load、initialize、activate、deactivate、shutdown、unload。
- [ ] Module：实现 binary hash、ABI version、packaged safety、release gate integration。
- [ ] Module：实现 example runtime plugin、asset importer plugin、renderer provider plugin。
- [ ] Docs：编写 Plugin Authoring Guide、Module ABI Reference、Engine Module Slot Guide。

验收：

- 插件可在 Debug/Release 中加载、注册服务/扩展/slot provider、卸载并报告完整 diagnostics。
- 恶意或错误 descriptor 不会导致崩溃，只产生 release-gate-blocking diagnostics。
- ABI compatibility tests 覆盖旧插件、缺失 symbol、版本不兼容、权限不足。

## 6. Property And Reflection-lite Rebuild

目标：用轻量 PropertySystem 支撑 Inspector、schema、serialization、AI review、MCP field editing，而不复制 UE UObject。

- [ ] Property：定义 TypeDescriptor、PropertyDescriptor、enum、struct、array、map、tagged union、asset ref、localized text。
- [ ] Property：定义 flags：ai_editable、tool_generated、read_only、requires_review、runtime_only、editor_only。
- [ ] Property：实现 default value、validation rule、range、regex、dependency、custom validator。
- [ ] Property：实现 JSON Schema generation 和 schema versioning。
- [ ] Property：实现 type migration：rename、split、merge、default injection、deprecated field。
- [ ] Property：实现 inspector metadata：display name、category、tooltip、order、visibility condition。
- [ ] Property：实现 diff/audit support，用于 Review Queue 和 Release Gate。
- [ ] Docs：编写 Property System Guide、Schema Authoring Guide、Migration Cookbook。

验收：

- Component、Asset sidecar、Project config、AI policy 均可使用同一 property/schema 基础。
- 复杂 nested struct/array/map 可生成 schema、校验、迁移和 inspector metadata。
- AI-editable 字段边界可被 Release Gate 和 Review Queue 识别。

## 7. Scene And Actor Runtime Rebuild

目标：重建公开 Actor/Component 对象模型，支持真实项目生命周期、Prefab、稳定存档和调试。

- [ ] Scene：定义 World、Scene、Actor、Component 的 ownership 和 lifetime。
- [ ] Scene：定义稳定 ActorId、ActorTypeId、ActorName、ActorHandle generation。
- [ ] Scene：实现 actor lifecycle：spawn、activate、deactivate、destroy、deferred destroy、preview attach。
- [ ] Scene：实现 component lifecycle：construct、default apply、validate、activate、deactivate、serialize、migrate。
- [ ] Scene：实现 prefab/defaults：base prefab、override、variant、nested prefab policy。
- [ ] Scene：实现 Actor reference resolution：stable id、soft reference、missing target diagnostics。
- [ ] Scene：实现 Tag、Lifetime、Transform2D、Blackboard、ControlPolicy、StateMachine 基础组件。
- [ ] Scene：实现 local ECS pack boundary：sync-in、update、sync-out、snapshot，不暴露 entity。
- [ ] Scene：实现 inspector snapshot：Actor tree、Component data、lifecycle state。
- [ ] Docs：编写 Actor/Component Programming Guide、Prefab Guide、Scene Debugging Guide。

验收：

- 1000+ Actor 可 spawn/destroy/tick/snapshot，无 handle reuse 错误。
- Prefab override 可保存、迁移、diff、回滚。
- 存档不包含 C++ pointer、ECS entity、renderer/audio native handle 或 Editor-only object。

## 8. Runtime Core Rebuild

目标：建立 deterministic runtime 中心，支撑状态机、任务调度、事件、Director、Save/Replay。

- [ ] Runtime：定义 deterministic tick model：fixed update、variable presentation update、frame index、event sequence。
- [ ] Runtime：实现 RuntimeEvent：type id、category、source/target、sequence、timestamp policy、payload schema。
- [ ] Runtime：实现 EventBus：immediate、queued、deferred、priority、subscription lifetime、trace hook。
- [ ] Runtime：实现 StateMachineRuntime：definition asset、guard/action API、delayed event、timer、snapshot。
- [ ] Runtime：实现 Blackboard：scope、typed value、schema、diff、save policy。
- [ ] Runtime：实现 ControlPolicy：owner、channel、lock、interrupt、queue、reject、priority inheritance。
- [ ] Runtime：实现 Director：global phase、timeline lock、choice lock、AI permission window、conflict arbitration.
- [ ] Runtime：实现 task/coroutine scheduler：cancellable task、wait event、wait time、wait asset、save state。
- [ ] Runtime：实现 replay recorder/player：input/event/script/presentation hash comparison。
- [ ] Runtime：实现 RuntimeWorld facade，组合 Scene、EventBus、StateMachine、Director、Scheduler。
- [ ] Runtime：按 `docs/design/runtime-core.md` 实现 RuntimeEvent、Scheduler、StateMachine、Director、Save/Replay contract。
- [ ] Docs：编写 Runtime Programming Guide、Event Guide、StateMachine Guide、Save/Replay Guide。

验收：

- 相同 seed、相同 input、相同 package 在两次运行中产生一致 state hash 和 presentation command hash。
- Runtime 可以暂停、单步、恢复、保存、加载、replay。
- Director/ControlPolicy 能处理 story script、player choice、AI intent、timeline 的冲突。

## 9. Save / Load / Replay Rebuild

目标：实现生产级存档与回放，不只保存脚本 label 或变量。

- [ ] Save：定义 save container：header、engine version、project version、module versions、schema versions。
- [ ] Save：保存 World、Scene、Actor、Component、StateMachine、Blackboard、ControlPolicy、Director。
- [ ] Save：保存 ScriptRuntime state、Timeline state、FilterProfile state、resource overrides、random seed。
- [ ] Save：保存 AI committed output 和 runtime intent audit reference。
- [ ] Save：支持 module extension state，Legacy 仅作为 expansion extension state。
- [ ] Save：实现 migration：snapshot schema migration、component migration、module state migration。
- [ ] Save：实现 integrity：hash、compression option、diagnostics、partial failure policy。
- [ ] Replay：记录 input、runtime events、script decisions、choice selections、committed AI output。
- [ ] Replay：实现 deterministic comparison：state hash、event hash、presentation hash。
- [ ] Docs：编写 Save Format Reference、Replay Debugging Guide、Migration Guide。

验收：

- 保存后重启进程可恢复到同一 runtime state。
- 旧存档格式可迁移或输出明确不可迁移 diagnostics。
- Replay mismatch 可定位到 frame、event、actor、component 或 script command。

## 10. Asset Pipeline Rebuild

目标：建立从 canonical source 到 cooked package 的完整内容管线。

- [ ] Asset：实现 AssetId：native、virtual、foreign-*，稳定 parse、normalize、compare、hash。
- [ ] Asset：实现 VFS：directory mount、package mount、read-only mount、mount priority、diagnostics。
- [ ] Asset：实现 sidecar schema：id、type、source_path、display_name、tags、origin、license、cook、review、ai_generation。
- [ ] Asset：实现 AssetRegistry：scan、dependency graph、generated registry、incremental invalidation。
- [ ] Asset：实现 importer framework：image、audio、font、text、filter profile、script、timeline。
- [ ] Asset：实现 cooker framework：source hash、settings hash、derived data key、output manifest。
- [ ] Asset：实现 DerivedDataCache：local cache、clean policy、versioning、corruption recovery。
- [ ] Asset：实现 package manifest：asset table、dependency table、module table、hash、release profile。
- [ ] Asset：实现 package reader：streaming read、random access、diagnostics、mount.
- [ ] Asset：实现 hot reload：asset/script/filter/timeline invalidation and rollback。
- [ ] Asset：实现 project template descriptor：template id、runtime profile、default providers、seed content、wizard fields、acceptance commands。
- [ ] Asset：实现 asset import preset schema：source extensions、asset type、sidecar defaults、cook defaults、license/review policy。
- [ ] Asset：实现 AI draft sidecar 和 Review Queue item schema。
- [ ] Asset：按 `docs/design/asset-pipeline.md` 实现 Importer、Cooker、DDC、Package Manifest、Hot Reload 和 Asset Release Gate contract。
- [ ] Docs：编写 Asset Authoring Guide、Importer/Cooker Guide、Package Format Reference。

验收：

- Native sample 可从 source content cook 成 package，并只从 package 启动。
- Release Gate 可阻止 missing dependency、duplicate id、invalid license、unreviewed AI asset、illegal foreign copy。
- Cook 两次产物 hash 稳定，source 变化只 invalidates 相关 assets。

## 11. Media Runtime Rebuild

目标：把 Media 从 DTO 记录提升为真实可执行 2D runtime backend，同时保留 headless 验证能力。

- [ ] Media：定义 RHI-lite / Renderer2D abstraction：device、texture、target、batch、present，不暴露 native handle。
- [ ] Media：实现 HeadlessRenderer2D：record command、hash output、frame capture metadata。
- [ ] Media：实现 SDL or selected 2D backend：window surface、texture upload、sprite draw、UI rect、debug text。
- [ ] Media：实现 image decode/cook path：PNG/JPEG/WebP policy、texture format、atlas option。
- [ ] Media：实现 sprite batching：layer order、z/order key、state grouping、clip/scissor。
- [ ] Media：实现 text/font：font asset、font atlas、fallback、basic shaping、localization-friendly layout。
- [ ] Media：实现 audio：mixer、voice/music/SFX bus、streaming、volume routing、pause/resume、save state。
- [ ] Media：实现 video extension point，不阻塞 core runtime acceptance。
- [ ] Media：实现 RenderGraph：extract、sort、execute、capture、diagnose。
- [ ] Media：实现 executable FilterGraph：background、character、ui、text、final layer target。
- [ ] Media：实现 Timeline/Animation：keyframes、events、camera, easing、save/replay state。
- [ ] Media：按 `docs/design/media-runtime.md` 实现 Renderer2D/TextLayout/Audio provider contract 和 media release gate。
- [ ] Docs：编写 Renderer2D Guide、Text/Font Guide、Audio Guide、FilterGraph Guide、Timeline Guide。

验收：

- Runtime sample 可真实显示背景、角色、UI、文本并播放 voice/music/SFX。
- Headless output 可验证 render order、filter target、text layout command、audio command hash。
- Media backend 不泄漏 SDL/GPU/audio handle 到 public ABI。

## 12. Script Runtime Rebuild

目标：建立稳定 ScriptRuntimeHost，确保脚本只能通过授权 API、RuntimeEvent 和 Presentation API 影响世界。

- [ ] Script：定义 ScriptRuntimeHost：runtime registration、selection、load、step、run、debug、snapshot。
- [ ] Script：定义 Script API surface：world query、event emit、presentation request、asset ref、save-safe state。
- [ ] Script：实现 Astra Native Script parser：source location、diagnostics、label、jump、choice、variables、expressions。
- [ ] Script：实现 AST/IR：stable command stream、schema、debug symbols、source map。
- [ ] Script：实现 Lua runtime：sandbox、host API binding、permission, deterministic snapshot, debug hook。
- [ ] Script：实现 ScriptEventBridge：script command -> RuntimeEvent/VNEvent/PresentationCommand。
- [ ] Script：实现 hot reload：parse validate、state compatibility check、rollback on failure。
- [ ] Script：实现 debugger hooks：breakpoint、step、inspect variable、call stack、current command。
- [ ] Script：按 `docs/design/script-and-presentation.md` 实现 ScriptRuntimeHost、Native DSL、Lua、Graph/Timeline 和 PresentationCommand contract。
- [ ] Docs：编写 Script Programming Guide、Native DSL Reference、Lua Host API Guide、Script Debugging Guide。

验收：

- Native Script 和 Lua 可驱动同一 VN scene，并产生同等 runtime events。
- 脚本错误有文件、行列、command、suggested fix。
- Script snapshot/restore 和 replay comparison 稳定。

## 13. Presentation And AstraVN Rebuild

目标：在通用 runtime 之上重建 VN-first 垂直模块，不污染 Core。

- [ ] Presentation：定义 presentation command schema：sprite、text、ui、audio、filter、camera、timeline。
- [ ] Presentation：定义 Presentation Library provider API，支持模块扩展。
- [ ] AstraVN：定义 VN event schema：Background、Character、Dialogue、Choice、Audio、Timeline、Filter。
- [ ] AstraVN：定义预置 Actor：Scene、StoryDirector、DialogueSystem、ChoiceSystem、AudioSystem、FilterSystem、Character、Camera。
- [ ] AstraVN：定义预置 Component：character profile、emotion、dialogue participant、choice list、audio cue、camera、timeline。
- [ ] AstraVN：定义预置 StateMachine：Dialogue、Choice、CharacterPresentation、Background、Audio、Timeline、FilterProfile。
- [ ] AstraVN：实现 VN graph/timeline integration，不只支持线性 DSL。
- [ ] AstraVN：实现 choice lock、route state、dialogue history、backlog、skip/auto hooks。
- [ ] AstraVN：实现 package-launchable native sample project。
- [ ] Docs：编写 AstraVN Overview、Dialogue/Choice Guide、Character Presentation Guide、VN Sample Tutorial。

验收：

- 完整 native AstraVN sample 可无 Editor 启动、显示、选择、保存、恢复、replay。
- VN module 只依赖 Runtime、Scene、Asset、Media、Script public API。
- Dialogue、choice、character、background、audio、timeline、filter 状态进入 save/replay。

## 14. Creator Experience Rebuild

目标：建立 UE 级创作者入口，让新创作者可以从模板到打包发布完成闭环；Editor 作为工具层使用同一 Runtime，不成为 runtime 发布前置条件。

- [ ] Editor：实现 Project Wizard、Template Browser、creator-facing sample launcher。
- [ ] Editor：实现 Content Browser、Asset Import Wizard、dependency view、reference repair。
- [ ] Editor：实现 Actor Type Palette、Component Palette、Prefab/Variant Browser。
- [ ] Editor：实现 command palette、context menu、asset picker、layout preset、undo/redo、dirty state。
- [ ] Editor：实现 editor layout preset schema 和 user/project override。
- [ ] Editor：实现 component inspector metadata schema：category、order、visibility、validation、review flags。
- [ ] Editor：实现 graph/timeline source schema：node/track/event、source map、debug symbols、hot reload policy。
- [ ] Editor：实现 creator task loop：Template -> Project -> Content -> PIE -> Package。
- [ ] Editor：定义 editor/runtime connection：inspect、command、PIE、pause、step、resume。
- [ ] Editor：实现 Project Browser、Asset Browser、Scene Tree。
- [ ] Editor：实现 Inspector：Actor、Component、StateMachine、Asset、Script state。
- [ ] Editor：实现 Script Editor：diagnostics、source map、breakpoint、run/step。
- [ ] Editor：实现 Graph/Timeline/FilterGraph Editor，输出 canonical source。
- [ ] Editor：实现 Runtime Debugger：event log、queued events、ControlPolicy locks、Director state。
- [ ] Editor：实现 Save/Replay Inspector：snapshot tree、diff、replay mismatch.
- [ ] Editor：实现 Asset Dependency Inspector 和 Cook/Package panel。
- [ ] Editor：实现 Output Log、Diagnostics panel、Profiler trace viewer。
- [ ] Docs：编写 Editor Manual、PIE Guide、Runtime Debugger Guide、Inspector Guide。

验收：

- Editor PIE 与 packaged runtime 使用同一 RuntimeWorld、ScriptRuntimeHost、AssetRegistry、Media pipeline。
- Editor 可暂停、单步、查看、保存、加载、replay 一个 running world。
- 关闭 Editor 后，packaged runtime 仍可完整运行 sample。

## 15. Customization Framework Rebuild

目标：让插件作者和工具作者拥有 UE 级可定制入口。

- [ ] Customization：实现 Plugin Wizard，生成 descriptor、C ABI stub、test、manual stub、release checklist。
- [ ] Customization：实现 plugin wizard template descriptor schema。
- [ ] Customization：实现 EngineModuleSlot selection UI 和 project policy validation。
- [ ] Customization：实现 `IEditorPanelProvider`、菜单、命令、context action、layout metadata。
- [ ] Customization：实现 provider templates：Renderer2D、TextLayout、Audio、ScriptRuntime、PresentationLibrary。
- [ ] Customization：实现 `IAssetImporter`、`ICookProcessor`、`IMcpToolProvider`、`IAIProvider` sample plugins。
- [ ] Customization：实现 release gate：capability、permission、packaged eligibility、ABI compatibility、binary hash。

验收：

- 插件作者可创建 Editor panel、asset importer、renderer/text/audio provider，并在 sample project 中启用。
- Project policy 可选择 provider，错误选择产生可修复 diagnostics。
- 插件不能跨 ABI 暴露 C++ ownership、Editor widget 或 native handles。

## 16. AI MCP Collaboration And Runtime Safety Rebuild

目标：Runtime AI MCP、Editor Copilot MCP、Editor Content Generation MCP 分离实现，不成为 Core 依赖。

Runtime AI MCP：

- [ ] AI：实现 Runtime MCP Host：runtime feedback、context inspect、intent request、intent validate、intent commit、fallback select。
- [ ] AI：实现 Runtime AI resources：runtime snapshot、player feedback、director state、allowed intent schema、fallback catalog。
- [ ] AI：实现 Runtime AI tools：feedback.submit、intent.request、intent.validate、intent.commit、fallback.select。
- [ ] AI：实现 Runtime AI save/replay：committed output、intent audit ref、provider-free replay。
- [ ] AI：实现 Runtime AI release policy：deterministic build 默认阻止 runtime provider，显式 release profile 才允许。

Editor Copilot MCP：

- [ ] AI：实现 Editor Copilot MCP：inline suggestion、patch proposal、diagnostics explanation、test/cook/release gate assistance。
- [ ] AI：实现 Copilot resources：project tree、selected source、diagnostics、schema、test report、cook report。
- [ ] AI：实现 Copilot tools：suggest、explain diagnostic、create patch proposal、run validation、prepare review item。
- [ ] AI：实现 Copilot write policy：默认 Review Queue；trusted session 才能直接 apply structured patch。

Editor Content Generation MCP：

- [ ] AI：实现 Editor Content Generation MCP：generate/modify/enhance draft、preview、variant compare、review/import。
- [ ] AI：实现 Content Generation resources：asset metadata、style refs、lore refs、license policy、draft workspace。
- [ ] AI：实现 Content Generation tools：draft.generate、draft.modify、draft.enhance、draft.preview、draft.accept、draft.reject。
- [ ] AI：实现 AI draft sidecar：provider、prompt/context/output hash、provenance、license、review target。

Shared safety：

- [ ] AI：实现 Boundary Manager：project policy、stage policy、allowed operations、review policy、release mode。
- [ ] AI：实现 Context Builder：source context、asset metadata、runtime snapshot、privacy/filter policy。
- [ ] AI：实现 Review Queue：patch、asset draft、localization draft、runtime intent preview。
- [ ] AI：实现 Operation Log 和 Generation Audit Log。
- [ ] AI：实现 Provider module interface：capabilities、network/offline、packaged eligibility、secret access。
- [ ] AI：实现 Asset Generation draft flow：temporary output、sidecar draft、preview、accept/reject。
- [ ] AI：实现 Runtime AIIntent：structured intent、validator、Director/ControlPolicy integration、committed output。
- [ ] AI：实现 deterministic committed output：save/replay uses committed data, never re-queries model.
- [ ] Docs：编写 Runtime AI MCP Guide、Editor Copilot MCP Guide、Editor Content Generation MCP Guide、Provider Authoring Guide、Audit Guide。

验收：

- AI 不能直接写 Cooked、DerivedDataCache、package manifest 或 foreign mount-only assets。
- Runtime MCP 不能 project write；Editor mutating tools 必须 trusted session 或 review。
- Editor Content Generation draft 未 accepted 前不能进入 Cook。
- Deterministic build 阻止未审核 AI 内容和未授权 runtime AI provider。
- Runtime AI intent 可保存、回放、审计、禁用。

## 17. Developer Tools, Release Gate And Observability Rebuild

目标：建立 UE-class runtime 所需的工具链、验证门禁和可观测性。

- [ ] Tools：实现 `astra validate`：schema、descriptor、project config、asset refs、script compile。
- [ ] Tools：实现 `astra cook`：incremental cook、DDC、package manifest、diagnostics。
- [ ] Tools：实现 `astra package`：deterministic package、module inclusion、hash report。
- [ ] Tools：实现 `astra run`：launch cooked package、headless run、scripted input。
- [ ] Tools：实现 `astra replay`：run replay, compare hashes, emit mismatch report。
- [ ] Tools：实现 `astra inspect`：package info、asset registry、module table、save file summary。
- [ ] Tools：实现 `astra doc-check`：links、required pages、code snippets、public API coverage。
- [ ] Release Gate：实现 blocking/non-blocking diagnostics policy。
- [ ] Release Gate：实现 plugin ABI、permissions、packaged eligibility、binary hash checks。
- [ ] Observability：实现 profiler markers、trace capture、frame timing、asset load timing、script timing。
- [ ] Observability：实现 crash/error report bundle：logs、diagnostics、build info、last frames。
- [ ] Tools：按 `docs/design/tools-release-observability.md` 实现 CLI output、validation report、package report、release gate report、trace 和 crash bundle contract。
- [ ] Docs：编写 CLI Tools Reference、Release Gate Guide、Profiling Guide、Crash Diagnostics Guide。

验收：

- Native sample 可通过 validate -> cook -> package -> run -> replay -> inspect 全链路。
- Release build 可生成 package report、diagnostics report、trace capture。
- 文档检查与测试门禁在 CI 中运行。

## 18. Samples And Test Matrix Rebuild

目标：用样例项目驱动引擎完备性，而不是只靠单元测试。

- [ ] Sample：`NativeVN`：背景、角色、对白、选择、音频、timeline、filter、save/replay。
- [ ] Sample：`RuntimeStress`：1000+ Actor、多状态机、多事件、多资源加载。
- [ ] Sample：`PackageSmoke`：只从 cooked package 启动。
- [ ] Sample：`ScriptParity`：Native Script 与 Lua 产生同等事件流。
- [ ] Sample：`MediaBackend`：真实 renderer/text/audio/filter output。
- [ ] Sample：`AIIntentSafety`：runtime intent validation、committed output、replay。
- [ ] Sample：`CreatorWorkflow`：template、asset import/generation、script/graph/timeline、PIE、package。
- [ ] Sample：`CustomizationPlugin`：Editor panel、asset importer、renderer/text/audio provider。
- [ ] Test：Core unit、Property unit、Module ABI unit、Scene unit、Runtime unit。
- [ ] Test：Asset integration、Cook/package integration、Media headless/render integration。
- [ ] Test：Save migration、Replay determinism、Plugin compatibility、Release Gate blocking。
- [ ] Test：Long-run soak、large-content stress、hot reload rollback、crash/error recovery。
- [ ] Docs：每个 sample 有 tutorial、expected output、troubleshooting、release checklist。
- [ ] Docs：按 `docs/design/samples-and-test-matrix.md` 为每个 sample 建立 descriptor、golden evidence 和 CI command。

验收：

- 每个 sample 可本地运行，也可在 CI/headless 模式验证核心行为。
- UE-class acceptance 只以完整 sample project 通过 release gate 作为完成证据。
- 所有 sample 文档和命令保持最新。
- `implementation-coverage.md` 中的每个系统都有 design、contract、TODO 和 evidence 映射。

## 19. UE-class 2D Runtime Acceptance Gate

目标：定义最终达标门槛，防止“功能看起来有了”但工程上不能发布。

- [ ] Acceptance：`NativeVN` 从 source content 完成 validate、cook、package。
- [ ] Acceptance：packaged `NativeVN` 在无 Editor 环境 launch、play、choice、save、load、replay。
- [ ] Acceptance：runtime 支持真实 image/font/audio rendering 和 executable FilterGraph。
- [ ] Acceptance：runtime 支持 deterministic script execution、event order、state hash、presentation hash。
- [ ] Acceptance：runtime 支持 module/plugin load、permission check、ABI validation、release-safe inclusion。
- [ ] Acceptance：runtime 支持 diagnostics、profiling、trace、crash/error report。
- [ ] Acceptance：Editor 可连接同一 runtime world 做 PIE/debug，但 runtime 不依赖 Editor。
- [ ] Acceptance：Creator workflow 支持模板、导入/生成资产、Graph/Timeline/Script、PIE、打包发布。
- [ ] Acceptance：Customization workflow 支持插件替换 renderer/text/audio 或添加 Editor panel/MCP tool。
- [ ] Acceptance：AI workflows 覆盖 Runtime AI MCP、Editor Copilot MCP、Editor Content Generation MCP。
- [ ] Acceptance：documentation manual 覆盖所有 public systems，doc-check 通过。
- [ ] Acceptance：stress/soak/migration/replay/package tests 通过。
- [ ] Acceptance：所有非目标仍成立：无复杂 3D/FPS/open-world/UObject parity 扩张。

验收：

- `astra validate Samples/NativeVN`
- `astra cook Samples/NativeVN --config Release`
- `astra package Samples/NativeVN --deterministic`
- `astra run Saved/Cooked/NativeVN --headless-smoke`
- `astra replay Saved/Replays/NativeVNGolden.replay --compare`
- `astra inspect Saved/Packages/NativeVN.astrapkg`
- `astra doc-check`
- `ctest --test-dir build -C Release --output-on-failure`

## 20. Expansion Track：Legacy Compatibility After Parity

目标：在 native runtime 达标后接入旧 VN 模拟和现代化，不反向污染 Core。

- [ ] Expansion：定义 CompatRuntimeProvider、ForeignProjectProbe、PackageReader、LegacyAssetResolver。
- [ ] Expansion：定义 Legacy VM state、opcode/timeline adapter、LegacyApiMapper。
- [ ] Expansion：定义 SaveExtensionStateProvider，进入 save/replay extension state。
- [ ] Expansion：定义 Compatibility Inspector 和 modernization profile。
- [ ] Expansion：实现 mock legacy runtime fixture。
- [ ] Expansion：实现 BGI/Kirikiri/Ren'Py/NScripter prototype 之一。
- [ ] Expansion：实现 Artemis prototype：unpacked-directory probe、`foreign-artemis:/` resolver、`.iet/.asb/.ast` index、minimal `e:*` host API。
- [ ] Expansion：实现 Artemis tag/API coverage report 和 high-frequency tag mapper。
- [ ] Expansion：实现 mount-only release gate 和 external asset diagnostics。
- [ ] Docs：编写 Legacy Compatibility Guide、Modernization Guide、Compat Plugin Authoring Guide。

验收：

- Mock legacy runtime 通过稳定 Runtime/Asset/Media/Script API 输出 Presentation events。
- Artemis prototype 输出 AstraVN Events，不把 Artemis VM 控制流并入 AstraVN source language。
- Legacy save extension state 不污染 native save model。
- Mount-only 默认不复制外部原始资产。
- Compat 模块不能要求修改 Core、Runtime、Asset、Media 的基础边界。
