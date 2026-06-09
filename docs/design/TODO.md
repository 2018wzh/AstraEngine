# TODO


## 1. UE-style Documentation System First

目标：开发与文档同步进行，形成类似 UE 文档的信息架构，避免实现先行后补文档。

- [x] Docs：建立 `docs/manual`，作为面向引擎使用者的开发文档根目录。
- [x] Docs：建立 `docs/manual/getting-started`：安装、构建、创建项目、运行 sample、打包。
- [x] Docs：建立 `docs/manual/programming`：Core、Module、Actor/Component、RuntimeEvent、StateMachine、Asset、Media、Script、AstraVN。
- [x] Docs：建立 `docs/manual/systems`：Asset Pipeline、Cook/Package、Save/Replay、Renderer2D、Text/Font、Audio、FilterGraph、Hot Reload、Diagnostics。
- [x] Docs：建立 `docs/manual/api`：public headers 的稳定 API reference 索引。
- [x] Docs：建立 `docs/manual/editor`：Editor、PIE、Inspector、Debugger、Review Queue。
- [x] Docs：建立 `docs/manual/samples`：完整 native AstraVN sample 的逐步教程。
- [x] Docs：建立 `docs/manual/migration`：snapshot/schema/plugin ABI 迁移指南。
- [x] Docs：建立 `docs/manual/release-notes`：每个 milestone 的变更、破坏性改动、验证命令。
- [x] Docs：建立 `docs/manual/concepts`：Astra 与 UE runtime parity 的对标边界和非目标。
- [x] Docs：为文档增加链接检查、过期检查、代码示例编译或片段验证策略。
- [x] Design：保持 `runtime-core.md`、`media-runtime.md`、`script-and-presentation.md` 与实现同步。
- [x] Design：保持 `foundation-core-platform-property.md`、`asset-pipeline.md`、`tools-release-observability.md` 与实现同步。
- [x] Design：保持 `samples-and-test-matrix.md` 和 `implementation-coverage.md` 与验收证据同步。
- [x] Design：每个 public runtime/editor/tool contract 必须在 design doc、manual page、schema/test 中至少各有一个权威引用。

Phase 1 注记：`docs/manual` 当前包含 Phase 1 Foundation 手册页和仍处于计划中的后续系统骨架。`tools/doc-check.ps1` 负责页面、链接和过期措辞检查；Foundation public API 覆盖和 schema/test evidence 已由 `Astra_Phase1Tests` 补齐，后续系统的代码示例编译和 release evidence 将在对应实现阶段继续补齐。

验收：

- 新模块 landing page 必须包含：Overview、Key Concepts、Architecture、Programming Guide、API Reference、Examples、Troubleshooting。
- 每个 milestone 合并前必须更新 manual、design、development、release notes。
- 不允许只有 TODO 或 ADR，没有面向开发者的操作文档。

## 2. Rebuild Foundation：Repository And Build System

目标：建立工程骨架，使生产 runtime、Editor、工具、测试和 sample 的边界从一开始清晰。

Phase 1 注记：当前已建立 runtime foundation targets、示例插件、Phase 1 tests、构建选项边界、`Astra_Tools` 和 `astra` foundation CLI。`validate`、`cook`、`package`、`run --headless-smoke`、`inspect`、`doc-check` 已覆盖 Phase 0/1 文档、Foundation smoke 和 `foundation_core_gate`；真实 asset cook、package launch、replay、full runtime release gate 仍归后续 Tools/Samples 阶段。

- [x] Build：定义新顶层目标命名规则：`Astra_<Module>`、`Astra_<Tool>`、`Astra_<Sample>`、`Astra_<TestSuite>`。
- [x] Build：拆分 runtime、editor、developer tools、samples、tests、plugins 的 CMake option。
- [x] Build：建立 Debug、RelWithDebInfo、Release 三种默认配置和 artifact 输出规范。
- [x] Build：建立 platform abstraction 层构建矩阵，首批至少 Windows + headless CI。
- [x] Build：建立 third-party dependency policy：允许、封装边界、ABI 暴露禁令、版本锁定。
- [x] Build：建立 generated/cooked/cache 目录约束，禁止源树污染。
- [x] Build：建立 `AstraBuildInfo`：版本、git commit、build config、feature flags、ABI version。
- [x] Build：建立 CLI 工具入口：`astra`，包含 validate、cook、package、run、inspect、doc-check。（Phase 1 为 foundation-only 实现。）
- [x] Build：建立 sample project 目录规范：`Samples/NativeVN`, `Samples/RuntimeStress`, `Samples/PackageSmoke`。
- [x] Test：建立测试分类和命名：unit、integration、headless、smoke、stress、compat、release-gate。

验收：

- 空构建、只构建 runtime、只构建 tools、构建 tests、构建 samples 均可独立完成。
- 构建产物不要求 Editor 存在即可运行 runtime sample。
- `astra --version` 输出 build info 和 enabled module list。

## 3. Core Rebuild

目标：建立稳定基础层，支撑 UE-class runtime 所需的诊断、配置、序列化、版本迁移和文档化 API。

Phase 1 注记：当前已实现 Core production Foundation gate slice：基础类型、`Result`、diagnostics、diagnostic code registry、release profile policy、logging sinks、assert/error policy facade、config layering/profile hash、release config resolve、time/path、versioned document/migration、unknown-field policy、stable IDs、profiling marker capture、build info 和 manual/API/CLI gate evidence。生产级外部 profiling backend export 仍属后续 hardening。

- [x] Core：定义基础类型策略：固定宽度整数、UTF-8、path、span/string view、expected/result、error code。
- [x] Core：重建 diagnostics：category、severity、code、source location、context object、machine-readable payload。
- [x] Core：重建 logging：channel、sink、structured fields、runtime/editor/tool routing、file rotation。
- [x] Core：重建 assert/error policy：developer assert、recoverable runtime error、fatal error、release behavior。
- [x] Core：重建 config：project config、runtime config、platform overrides、module policy、release profile。
- [x] Core：重建 time：monotonic clock、game time、real time、fixed step、pausable timers、serialized timer state。
- [x] Core：重建 path/file utility：canonical project path、package path、user save path、cache path。
- [x] Core：实现 serialization framework：versioned document、schema id、migration registry、unknown field policy。
- [x] Core：实现 stable id framework：TypeId、PropertyId、AssetId、ActorId、ComponentId、EventTypeId。
- [x] Core：实现 telemetry/profiling marker API，不依赖 Editor。
- [x] Core：按 `docs/design/foundation-core-platform-property.md` 实现 diagnostics、config、serialization、stable id 和 PropertySystem contract。
- [x] Docs：编写 Core Programming Guide、Diagnostics Guide、Serialization/Migration Guide。

验收：

- Core 无 SDL、Lua、AI、VN、Editor、renderer、audio 依赖。
- Diagnostics 可被 CLI、Runtime、Editor、Release Gate 统一消费。

## 4. Platform Rebuild

目标：提供 runtime 可发布所需的平台抽象，同时 public API 不泄漏 SDL 或 OS handle。

Phase 1 注记：当前已实现 Platform public service interfaces、headless backend、SDL-backed window backend compile path、filesystem/timer/thread/dynamic-library/crash service foundation、opaque dynamic library handles、file-watch polling、pending task tags、crash capture context 和 public header isolation tests。完整 input service、worker pool implementation、crash minidump 和 SDL sample 行为覆盖仍属后续阶段。

- [x] Platform：定义 window service、monitor/DPI、clipboard、cursor、display mode。
- [x] Platform：定义 input service：keyboard、mouse、text input、gamepad、touch extension point。
- [x] Platform：定义 filesystem service：project mount、user save、cache、package read、watch。
- [x] Platform：定义 dynamic library service：load、symbol、version check、safe unload policy。
- [x] Platform：定义 thread service：worker pool、main thread dispatch、job tags、shutdown order。
- [x] Platform：定义 timer service：high-resolution time、sleep/yield、frame pacing hooks。
- [x] Platform：定义 crash/error hooks：minidump path、last log capture、fatal diagnostic packet。
- [x] Platform：实现 headless backend，作为 CI 和 server-style runtime 验证环境。
- [x] Platform：实现 SDL-backed backend，所有 SDL 类型限制在 private implementation。
- [x] Platform：按 `docs/design/foundation-core-platform-property.md` 实现 headless/SDL backend、filesystem/thread/timer/crash service contract。
- [x] Docs：编写 Platform Programming Guide 和 Backend Porting Guide。

验收：

- Headless sample 不创建窗口也能运行 runtime tick、save/replay、cook/package validation。
- SDL sample 可创建窗口、输入事件、退出事件，但 public headers 不包含 SDL 类型。
- 文件监听、动态库加载和线程池有错误恢复测试。

## 5. Module Runtime Rebuild

目标：建立类似 UE module/plugin 体系的运行时扩展能力，但以 C ABI 和显式权限为稳定边界。

Phase 1 注记：当前已实现 descriptor parsing/validation、dependency resolver、C ABI、module lifecycle、service/extension/provider registry、service resolve audit、engine module slot policy validation、example foundation plugin load/unload test，以及 foundation release-gate binary/ABI/packaged-safety report。完整 semver range solver、hot reload、provider-specific contracts 和 plugin wizard 仍属后续阶段。

- [x] Module：实现 plugin descriptor schema：id、version、api range、modules、dependencies、capabilities、permissions、packaged eligibility。
- [x] Module：按 `docs/design/extension-and-module-system.md` 实现 descriptor validation、C ABI lifetime、ServiceRegistry、ExtensionRegistry、EngineModuleSlot contract。
- [x] Module：实现 descriptor validator，输出 machine-readable diagnostics。
- [x] Module：实现 dependency resolver：version range、load phase、optional dependency、cycle diagnostics。
- [x] Module：实现 C ABI：host API、module API、lifecycle、diagnostics、opaque handles。
- [x] Module：实现 ServiceRegistry：service id、capability view、permission gate、lifetime owner、resolve audit。
- [x] Module：实现 ExtensionRegistry：extension kind、descriptor payload、duplicate policy、query filter。
- [x] Module：实现 EngineModuleSlot：slot、provider、default provider、project selection、release policy。
- [x] Module：实现 module lifecycle：discover、validate、load、initialize、activate、deactivate、shutdown、unload。
- [x] Module：实现 binary hash、ABI version、packaged safety、release gate integration。（Phase 1 为 foundation release-gate checks。）
- [x] Module：实现 example runtime plugin、asset importer plugin、renderer provider plugin。
- [x] Docs：编写 Plugin Authoring Guide、Module ABI Reference、Engine Module Slot Guide。

验收：

- 插件可在 Debug/Release 中加载、注册服务/扩展/slot provider、卸载并报告完整 diagnostics。
- 恶意或错误 descriptor 不会导致崩溃，只产生 release-gate-blocking diagnostics。
- ABI compatibility tests 覆盖旧插件、缺失 symbol、版本不兼容、权限不足。

## 6. Property And Reflection-lite Rebuild

目标：用轻量 PropertySystem 支撑 Inspector、schema、serialization、AI review、MCP field editing，而不复制 UE UObject。

Phase 1 注记：当前已实现 TypeDescriptor、PropertyDescriptor、flags、defaults、validation metadata、inspector metadata、nested JSON Schema generation、type registry、custom validator registry、schema version graph、write policy、diff/audit output 和 migration helper。完整 Review Queue/Editor/MCP consumers 仍属后续阶段。

- [x] Property：定义 TypeDescriptor、PropertyDescriptor、enum、struct、array、map、tagged union、asset ref、localized text。
- [x] Property：定义 flags：ai_editable、tool_generated、read_only、requires_review、runtime_only、editor_only。
- [x] Property：实现 default value、validation rule、range、regex、dependency、custom validator。
- [x] Property：实现 JSON Schema generation 和 schema versioning。
- [x] Property：实现 type migration：rename、split、merge、default injection、deprecated field。
- [x] Property：实现 inspector metadata：display name、category、tooltip、order、visibility condition。
- [x] Property：实现 diff/audit support，用于 Review Queue 和 Release Gate。
- [x] Docs：编写 Property System Guide、Schema Authoring Guide、Migration Cookbook。

验收：

- Component、Asset sidecar、Project config、AI policy 均可使用同一 property/schema 基础。
- 复杂 nested struct/array/map 可生成 schema、校验、迁移和 inspector metadata。
- AI-editable 字段边界可被 Release Gate 和 Review Queue 识别。

## 7. Scene And Actor Runtime Rebuild

目标：重建公开 Actor/Component 对象模型，支持真实项目生命周期、Prefab、稳定存档和调试。

Phase 2 注记：当前已实现 foundation `Astra_Scene`：`ActorWorld`、stable Actor/Component DTO、`ComponentDescriptor`、generation-safe `ActorHandle`、spawn/activate/deactivate/destroy、foundation component descriptors、JSON component data、world snapshot/restore、missing actor reference diagnostics、headless local ECS pack 和 private EnTT-backed storage。EnTT 是 private implementation detail，不进入 public ABI、save ID、Editor/MCP contract 或 authoring model。完整 prefab/variant、deferred destroy、preview attach、component migration hardening 和 production inspector/debugger 仍属后续 completion。

- [x] Scene：定义 World、Scene、Actor、Component 的 ownership 和 lifetime。（Phase 2 foundation：`ActorWorld` ownership。）
- [x] Scene：定义稳定 ActorId、ActorTypeId、ActorName、ActorHandle generation。
- [ ] Scene：实现 actor lifecycle：spawn、activate、deactivate、destroy、deferred destroy、preview attach。（Phase 2 foundation 覆盖 spawn/activate/deactivate/destroy；deferred destroy 和 preview attach 后续完成。）
- [ ] Scene：实现 component lifecycle：construct、default apply、validate、activate、deactivate、serialize、migrate。（Phase 2 foundation 覆盖 `ComponentDescriptor`、default data 和 serialize；validation/migration hardening 后续完成。）
- [ ] Scene：实现 prefab/defaults：base prefab、override、variant、nested prefab policy。
- [ ] Scene：实现 Actor reference resolution：stable id、soft reference、missing target diagnostics。（Phase 2 foundation 覆盖 stable handle diagnostics 和 missing target diagnostics；soft reference repair 后续完成。）
- [x] Scene：实现 Tag、Lifetime、Transform2D、Blackboard、ControlPolicy、StateMachine 基础组件。
- [x] Scene：实现 local ECS pack boundary：sync-in、update、sync-out、snapshot，不暴露 entity。
- [x] Scene：实现 inspector snapshot：Actor tree、Component data、lifecycle state。
- [ ] Docs：编写 Actor/Component Programming Guide、Prefab Guide、Scene Debugging Guide。（Phase 2 foundation 提供 Actor/Component guide；Prefab/Scene Debugging 专页后续随 production 功能补齐。）

验收：

- 1000+ Actor 可 spawn/destroy/tick/snapshot，无 handle reuse 错误。
- Prefab override 可保存、迁移、diff、回滚。
- 存档不包含 C++ pointer、ECS entity、renderer/audio native handle 或 Editor-only object。

## 8. Runtime Core Rebuild

目标：建立 deterministic runtime 中心，支撑状态机、任务调度、事件、Director、Save/Replay。

Phase 2 注记：当前已实现 foundation `Astra_Runtime`：`RuntimeWorld`、frame/fixed-step counters、RuntimeEvent DTO、immediate/queued/deferred EventBus、trace、actor-bound state-machine transition、ControlPolicy allow/queue/reject foundation、Director state、foundation save/load、RuntimeReplay DTO 和 deterministic stable hash smoke。完整 scheduler/coroutine、guard/action API、priority/subscription lifetime、presentation extraction、production replay mismatch 定位和 full Director arbitration 仍属后续 completion。

- [ ] Runtime：定义 deterministic tick model：fixed update、variable presentation update、frame index、event sequence。（Phase 2 foundation 覆盖 frame/fixed-step/event sequence 和 stable hash；variable presentation 后续 Media/Runtime completion。）
- [x] Runtime：实现 RuntimeEvent：type id、category、source/target、sequence、timestamp policy、payload schema。
- [ ] Runtime：实现 EventBus：immediate、queued、deferred、priority、subscription lifetime、trace hook。（Phase 2 foundation 覆盖 immediate/queued/deferred/trace；priority/subscription lifetime 后续完成。）
- [ ] Runtime：实现 StateMachineRuntime：definition asset、guard/action API、delayed event、timer、snapshot。（Phase 2 foundation 覆盖 registered definition 和 transition snapshot；guard/action/delayed timer 后续完成。）
- [ ] Runtime：实现 Blackboard：scope、typed value、schema、diff、save policy。（Phase 2 foundation 覆盖 Blackboard component JSON 和 save；scope/schema/diff 后续完成。）
- [ ] Runtime：实现 ControlPolicy：owner、channel、lock、interrupt、queue、reject、priority inheritance。（Phase 2 foundation 覆盖 allow/queue/reject decision；interrupt 和 priority inheritance 后续完成。）
- [ ] Runtime：实现 Director：global phase、timeline lock、choice lock、AI permission window、conflict arbitration.（Phase 2 foundation 覆盖 Director state；conflict arbitration 后续完成。）
- [ ] Runtime：实现 task/coroutine scheduler：cancellable task、wait event、wait time、wait asset、save state。
- [ ] Runtime：实现 replay recorder/player：input/event/script/presentation hash comparison。（Phase 2 foundation 覆盖 RuntimeReplay DTO、event trace 和 state/event/presentation hash smoke；input/script/presentation comparison 和 mismatch 定位后续完成。）
- [ ] Runtime：实现 RuntimeWorld facade，组合 Scene、EventBus、StateMachine、Director、Scheduler。（Phase 2 foundation 组合 Scene/EventBus/StateMachine/Director；Scheduler 后续完成。）
- [ ] Runtime：按 `docs/design/runtime-core.md` 实现 RuntimeEvent、Scheduler、StateMachine、Director、Save/Replay contract。（Phase 2 foundation 覆盖 RuntimeEvent/StateMachine/Director/SaveReplay 基础；Scheduler 和 production contract 后续完成。）
- [x] Docs：编写 Runtime Programming Guide、Event Guide、StateMachine Guide、Save/Replay Guide。

验收：

- 相同 seed、相同 input、相同 package 在两次运行中产生一致 state hash 和 presentation command hash。
- Runtime 可以暂停、单步、恢复、保存、加载、replay。
- Director/ControlPolicy 能处理 story script、player choice、AI intent、timeline 的冲突。

## 9. Save / Load / Replay Rebuild

目标：实现生产级存档与回放，不只保存脚本 label 或变量。

Phase 2 注记：当前已实现 foundation save/load/replay：`astra.runtime.snapshot.v1` `VersionedDocument`、`astra.runtime.replay.v1` DTO、frame/fixed-step/event sequence、seed、world/actor/component/state-machine/blackboard/control-policy/director snapshot、runtime event trace 和 deterministic stable hash smoke。生产级 save header、module/schema version matrix、script/timeline/filter/resource/AI/module extension state、migration/integrity/compression 和 replay mismatch report 仍属后续 completion。

- [ ] Save：定义 save container：header、engine version、project version、module versions、schema versions。（Phase 2 foundation 覆盖 snapshot schema/version；完整 header/version matrix 后续完成。）
- [x] Save：保存 World、Scene、Actor、Component、StateMachine、Blackboard、ControlPolicy、Director。
- [ ] Save：保存 ScriptRuntime state、Timeline state、FilterProfile state、resource overrides、random seed。
- [ ] Save：保存 AI committed output 和 runtime intent audit reference。
- [ ] Save：支持 module extension state，Legacy 仅作为 expansion extension state。
- [ ] Save：实现 migration：snapshot schema migration、component migration、module state migration。
- [ ] Save：实现 integrity：hash、compression option、diagnostics、partial failure policy。
- [ ] Replay：记录 input、runtime events、script decisions、choice selections、committed AI output。（Phase 2 foundation 覆盖 runtime event trace；input/script/choice/AI 后续完成。）
- [x] Replay：实现 deterministic comparison：state hash、event hash、presentation hash。
- [ ] Docs：编写 Save Format Reference、Replay Debugging Guide、Migration Guide。（Phase 2 foundation 提供 Save/Replay guide；格式参考、debugging、migration 专页后续补齐。）

验收：

- 保存后重启进程可恢复到同一 runtime state。
- 旧存档格式可迁移或输出明确不可迁移 diagnostics。
- Replay mismatch 可定位到 frame、event、actor、component 或 script command。

## 10. Asset Pipeline Rebuild

目标：建立从 canonical source 到 cooked package 的完整内容管线。

Phase 3 注记：当前已实现 `Astra_Asset` foundation slice：asset URI/ID 解析、VFS mount、sidecar DTO/validation、registry scan、dependency diagnostics、import preset/project template/review item DTO 和 watch invalidation plumbing。真实 importer/cooker/DDC/package reader、production hot reload rollback 和完整 Asset Release Gate 仍属 Phase 6 completion。

- [x] Asset：实现 AssetId：native、virtual、foreign-*，稳定 parse、normalize、compare、hash。（Phase 3 foundation 覆盖 `AssetUri` 和 Core `AssetId` kind；生产级 package identity 后续完成。）
- [x] Asset：实现 VFS：directory mount、package mount、read-only mount、mount priority、diagnostics。（Phase 3 foundation 覆盖 mount/resolve/read-only/priority；package reader mount 后续完成。）
- [x] Asset：实现 sidecar schema：id、type、source_path、display_name、tags、origin、license、cook、review、ai_generation。（Phase 3 foundation 覆盖 DTO 和 validation。）
- [x] Asset：实现 AssetRegistry：scan、dependency graph、generated registry、incremental invalidation。（Phase 3 foundation 覆盖 sidecar scan、hard dependency diagnostics 和 watch invalidation records；incremental cook invalidation 后续完成。）
- [ ] Asset：实现 importer framework：image、audio、font、text、filter profile、script、timeline。
- [ ] Asset：实现 cooker framework：source hash、settings hash、derived data key、output manifest。
- [ ] Asset：实现 DerivedDataCache：local cache、clean policy、versioning、corruption recovery。
- [ ] Asset：实现 package manifest：asset table、dependency table、module table、hash、release profile。
- [ ] Asset：实现 package reader：streaming read、random access、diagnostics、mount.
- [ ] Asset：实现 hot reload：asset/script/filter/timeline invalidation and rollback。
- [x] Asset：实现 project template descriptor：template id、runtime profile、default providers、seed content、wizard fields、acceptance commands。（Phase 3 foundation DTO/validation。）
- [x] Asset：实现 asset import preset schema：source extensions、asset type、sidecar defaults、cook defaults、license/review policy。（Phase 3 foundation DTO/validation。）
- [x] Asset：实现 AI draft sidecar 和 Review Queue item schema。（Phase 3 foundation sidecar fields 和 review item DTO/validation。）
- [ ] Asset：按 `docs/design/asset-pipeline.md` 实现 Importer、Cooker、DDC、Package Manifest、Hot Reload 和 Asset Release Gate contract。
- [x] Docs：编写 Asset Authoring Guide、Importer/Cooker Guide、Package Format Reference。（Phase 3 foundation manual covers authoring/VFS/sidecar; full cook/package references remain planned pages.)

验收：

- Native sample 可从 source content cook 成 package，并只从 package 启动。
- Release Gate 可阻止 missing dependency、duplicate id、invalid license、unreviewed AI asset、illegal foreign copy。
- Cook 两次产物 hash 稳定，source 变化只 invalidates 相关 assets。

## 11. Media Runtime Rebuild

目标：把 Media 从 DTO 记录提升为真实可执行 2D runtime backend，同时保留 headless 验证能力。

Phase 3 注记：当前已实现 `Astra_Media` foundation slice：PresentationCommand、RenderGraph/text/audio/filter DTO、FilterProfile validation/application、Renderer2D/TextLayout/Audio foundation provider descriptors、media release-gate foundation validation、HeadlessRenderer2D deterministic capture/hash 和 SDL renderer factory private compile-path stub。真实 image decode、font shaping、audio playback、GPU filter execution、timeline runtime 和 production provider replacement 仍属 Phase 7/10 completion。

- [x] Media：定义 RHI-lite / Renderer2D abstraction：device、texture、target、batch、present，不暴露 native handle。（Phase 3 foundation 覆盖 DTO renderer facade/headless capture；真实 device/texture 后续完成。）
- [x] Media：实现 HeadlessRenderer2D：record command、hash output、frame capture metadata。
- [x] Media：实现 SDL or selected 2D backend：window surface、texture upload、sprite draw、UI rect、debug text。（Phase 3 foundation 仅实现 SDL private compile-path factory stub；真实 renderer 后续完成。）
- [ ] Media：实现 image decode/cook path：PNG/JPEG/WebP policy、texture format、atlas option。
- [x] Media：实现 sprite batching：layer order、z/order key、state grouping、clip/scissor。（Phase 3 foundation 覆盖 layer/order sorting；真实 batching/state grouping/clip 后续完成。）
- [x] Media：实现 text/font：font asset、font atlas、fallback、basic shaping、localization-friendly layout。（Phase 3 foundation 覆盖 text layout request DTO/hash；真实 font atlas/shaping 后续完成。）
- [x] Media：实现 audio：mixer、voice/music/SFX bus、streaming、volume routing、pause/resume、save state。（Phase 3 foundation 覆盖 logical audio command DTO/hash；真实 playback/mixer/save state 后续完成。）
- [ ] Media：实现 video extension point，不阻塞 core runtime acceptance。
- [x] Media：实现 RenderGraph：extract、sort、execute、capture、diagnose。（Phase 3 foundation 覆盖 extract/sort/headless capture/diagnostics；真实 backend execute 后续完成。）
- [x] Media：实现 executable FilterGraph：background、character、ui、text、final layer target。（Phase 3 foundation 覆盖 layer-aware validation/application records；GPU execution 后续完成。）
- [ ] Media：实现 Timeline/Animation：keyframes、events、camera, easing、save/replay state。
- [x] Media：按 `docs/design/media-runtime.md` 实现 Renderer2D/TextLayout/Audio provider contract 和 media release gate。（Phase 3 foundation 覆盖 provider descriptor validation、required slot selection、packaged/headless eligibility 和 CLI evidence；真实 backend provider replacement/release gate 后续完成。）
- [x] Docs：编写 Renderer2D Guide、Text/Font Guide、Audio Guide、FilterGraph Guide、Timeline Guide。（Phase 3 foundation manual covers DTO/headless/filter; production backend guides remain planned sections.)

验收：

- Runtime sample 可真实显示背景、角色、UI、文本并播放 voice/music/SFX。
- Headless output 可验证 render order、filter target、text layout command、audio command hash。
- Media backend 不泄漏 SDL/GPU/audio handle 到 public ABI。

## 12. Script Runtime Rebuild

目标：建立稳定 ScriptRuntimeHost，确保脚本只能通过授权 API、RuntimeEvent 和 Presentation API 影响世界。

Phase 4 注记：当前已实现 `Astra_Script` foundation slice：`ScriptRuntimeHost`、provider descriptor、Native DSL parser、Lua provider via `sol2`、shared command stream、source location diagnostics、debug-symbol DTO、`ScriptSnapshot`、`ScriptEventBridge`、Native/Lua headless parity evidence。完整 debugger、hot reload rollback、Graph/Timeline compiler、production Lua continuation snapshot 和完整 Script API 权限系统仍属 Phase 8 completion。

- [x] Script：定义 ScriptRuntimeHost：runtime registration、selection、load、step、run、debug、snapshot。（Phase 4 foundation 覆盖 provider selection、compile/run 和 snapshot DTO；step/debug 后续完成。）
- [x] Script：定义 Script API surface：world query、event emit、presentation request、asset ref、save-safe state。（Phase 4 foundation 覆盖 event/presentation/asset ref/snapshot；world query 和完整权限 surface 后续完成。）
- [x] Script：实现 Astra Native Script parser：source location、diagnostics、label、jump、choice、variables、expressions。（Phase 4 foundation 覆盖 label/jump/choice/set/get、asset URI 和诊断；完整 expression language 后续完成。）
- [x] Script：实现 AST/IR：stable command stream、schema、debug symbols、source map。（Phase 4 foundation 覆盖 shared command DTO 和 debug-symbol DTO；完整 source map schema 后续完成。）
- [x] Script：实现 Lua runtime：sandbox、host API binding、permission, deterministic snapshot, debug hook。（Phase 4 foundation 使用 `sol2` 和最小 `astra` host API；完整 sandbox policy、permission validator、continuation snapshot 和 debug hook 后续完成。）
- [x] Script：实现 ScriptEventBridge：script command -> RuntimeEvent/VNEvent/PresentationCommand。（Phase 4 foundation 覆盖 Native/Lua 到 RuntimeEvent 和 PresentationCommand。）
- [ ] Script：实现 hot reload：parse validate、state compatibility check、rollback on failure。
- [ ] Script：实现 debugger hooks：breakpoint、step、inspect variable、call stack、current command。
- [ ] Script：按 `docs/design/script-and-presentation.md` 实现 ScriptRuntimeHost、Native DSL、Lua、Graph/Timeline 和 PresentationCommand contract。
- [ ] Docs：编写 Script Programming Guide、Native DSL Reference、Lua Host API Guide、Script Debugging Guide。（Phase 4 foundation covers Script、Native DSL、Lua Host API；debugging guide remains planned with debugger implementation。）

验收：

- Native Script 和 Lua 可驱动同一 VN scene，并产生同等 runtime events。
- 脚本错误有文件、行列、command、suggested fix。
- Script snapshot/restore 和 replay comparison 稳定。

## 13. Presentation And AstraVN Rebuild

目标：在通用 runtime 之上重建 VN-first 垂直模块，不污染 Core。

Phase 4 注记：当前已实现 `Astra_AstraVN` foundation slice：VN event schema、预设 Actor/Component/StateMachine、`VnSession`、`VnSessionSnapshot`、Native/Lua parity、headless presentation capture 和 save/restore evidence。完整 Presentation Library provider、Graph/Timeline integration、backlog、skip/auto、production package launch 和 production replay 仍属 Phase 8 completion。

- [x] Presentation：定义 presentation command schema：sprite、text、ui、audio、filter、camera、timeline。（Phase 4 foundation 复用 `Astra_Media::PresentationCommand` 并覆盖 sprite/text/ui/audio/filter；camera/timeline 为 schema identifier。）
- [ ] Presentation：定义 Presentation Library provider API，支持模块扩展。
- [x] AstraVN：定义 VN event schema：Background、Character、Dialogue、Choice、Audio、Timeline、Filter。（Phase 4 foundation 也包含 Camera schema identifier。）
- [x] AstraVN：定义预置 Actor：Scene、StoryDirector、DialogueSystem、ChoiceSystem、AudioSystem、FilterSystem、Character、Camera。（Phase 4 foundation 覆盖固定 `NativeVN` smoke preset。）
- [x] AstraVN：定义预置 Component：character profile、emotion、dialogue participant、choice list、audio cue、camera、timeline。（Phase 4 foundation 覆盖 DTO descriptors；完整 inspector metadata 后续完成。）
- [x] AstraVN：定义预置 StateMachine：Dialogue、Choice、CharacterPresentation、Background、Audio、Timeline、FilterProfile。（Phase 4 foundation 覆盖 basic event transitions；完整 route/lock/timeline state 后续完成。）
- [ ] AstraVN：实现 VN graph/timeline integration，不只支持线性 DSL。
- [ ] AstraVN：实现 choice lock、route state、dialogue history、backlog、skip/auto hooks。
- [ ] AstraVN：实现 package-launchable native sample project。（Phase 4 foundation 覆盖 `Samples/NativeVN` headless playable CLI smoke；真实 package launch 后续完成。）
- [ ] Docs：编写 AstraVN Overview、Dialogue/Choice Guide、Character Presentation Guide、VN Sample Tutorial。（Phase 4 foundation covers AstraVN overview and sample evidence；production tutorial pages remain planned。）

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

Phase 1/3/4 注记：`Astra_Tools` 和 `astra` 已实现 foundation-only `validate`、`cook`、`package`、`run --headless-smoke`、`inspect`、`doc-check`，并使用 CLI11、yaml-cpp、nlohmann_json、OpenSSL SHA-256、Lua、sol2 和 CTest/Catch2 提供证据。Phase 3 sample validate/headless smoke 会输出 Asset/Media/FilterGraph hash 和 media provider release-gate foundation evidence；Phase 4 NativeVN 会输出 Script/AstraVN headless evidence。下列条目描述的是完整 UE-class Tools/Release Gate/Observability 目标，仍需等待 Asset、RuntimeWorld、Script、Media、Replay 等系统实现后才能完成。

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
