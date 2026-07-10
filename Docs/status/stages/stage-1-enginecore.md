# Stage 1 EngineCore Work

Stage 1 已交付 EngineCore 的可运行闭环：UE 风格 workspace、核心类型、Runtime tick、StateMachine action provider、AwaitToken、DelayedEventQueue、Save/Replay、Plugin ABI、PropertySystem、Target manifest 和 headless scenario runner。后续 Stage 的页面仍记录未实现工作。

## S1-BOOT-01 Workspace 与基础 CI

**ID:** `S1-BOOT-01`

**Goal:** 建立 Rust workspace、固定 toolchain、基础 crate 边界和 CI 检查入口。

**Depends On:** `Docs/modules/engine-core.md`、`Docs/contracts/runtime.md`

**Target Paths:** `Cargo.toml`、`rust-toolchain.toml`、`Engine/Source/Runtime/astra-core/`、`Engine/Source/Runtime/astra-runtime/`、`Engine/Source/Runtime/astra-plugin/`、`Engine/Source/Developer/astra-property/`、`Engine/Source/Developer/astra-property-derive/`、`Engine/Source/Developer/astra-test/`、`Engine/Source/Programs/astra-cli/`、`Engine/Plugins/Fixtures/headless-presentation-provider/`、`.github/workflows/ci.yml`

**Steps:**

1. 新建 workspace manifest，启用 resolver 2，并登记 Stage 1 crate。
2. 固定 stable Rust toolchain，记录目标 host triple。
3. 给每个 crate 建 Stage 1 `lib.rs`，只暴露本 Stage 必需 public module。
4. 配置 CI 顺序：`python Tools/check_docs.py`、`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。
5. 本地执行同一组命令，保存命令输出作为合入证据。

**Done Evidence:** CI 配置存在，本地四条命令通过；crate graph 不包含 Editor、Luau、GPU/audio native handle 或 provider 实现。

**Linked Test IDs:** `T-S1-BOOT-01`

## S1-CORE-01 StableId、diagnostics 与 schema 基础类型

**ID:** `S1-CORE-01`

**Goal:** `astra-core` 提供跨模块共享的 StableId、Diagnostic、SchemaVersion、Hash128 和 Result 类型。

**Depends On:** `S1-BOOT-01`

**Target Paths:** `Engine/Source/Runtime/astra-core/src/id.rs`、`Engine/Source/Runtime/astra-core/src/diagnostic.rs`、`Engine/Source/Runtime/astra-core/src/schema.rs`、`Engine/Source/Runtime/astra-core/src/hash.rs`、`Engine/Source/Runtime/astra-core/tests/core_types.rs`

**Steps:**

1. 定义 StableId 的生成、解析、显示和 serde 形态。
2. 定义 Diagnostic severity、code、message、source span 和 machine-readable fields。
3. 定义 SchemaVersion、migration range 和 Hash128 newtype。
4. 用 BLAKE3 生成 tick/report `Hash128`，用 SHA-256 生成 container/binary `Hash256`。
5. 编写 roundtrip、display 和 serde 测试。

**Done Evidence:** `astra-core` 测试覆盖 id roundtrip、diagnostic serialization、schema ordering 和 hash repeatability。

**Linked Test IDs:** `T-S1-CORE-01`

## S1-RUNTIME-01 RuntimeWorld 与 Actor/Component

**ID:** `S1-RUNTIME-01`

**Goal:** `astra-runtime` 提供 RuntimeWorld、Actor、Component、Blackboard 和 RuntimeConfig 的 Stage 1 public API。

**Depends On:** `S1-CORE-01`

**Target Paths:** `Engine/Source/Runtime/astra-runtime/src/world.rs`、`Engine/Source/Runtime/astra-runtime/src/actor.rs`、`Engine/Source/Runtime/astra-runtime/src/blackboard.rs`、`Engine/Source/Runtime/astra-runtime/tests/world_actor.rs`

**Steps:**

1. 建立 RuntimeWorld create/tick/apply_input/save/load/debug 的 public facade。
2. 实现 ActorId、ComponentId、component metadata 和 actor lifecycle。
3. 让 Blackboard 使用可序列化值类型，不接受 native handle 或 Future。
4. 确认 RuntimeWorld 不依赖 Editor UI、MCP server、Luau runtime 或 renderer backend。
5. 编写 actor create/remove、component attach/detach 和 blackboard snapshot 测试。

**Done Evidence:** `world_actor` 与 `trigger_event` 覆盖 Actor lifecycle、schema/version/codec/hash/bytes typed component、typed replace、MutationLog、trigger event 和稳定 snapshot hash。

**Linked Test IDs:** `T-S1-RUNTIME-01`

## S1-RUNTIME-02 StateMachine、EventBus 与 fixed tick

**ID:** `S1-RUNTIME-02`

**Goal:** 实现同步 guard、ordered EventBus、fixed tick Scheduler 和可追踪 flat StateMachine。

**Depends On:** `S1-RUNTIME-01`

**Target Paths:** `Engine/Source/Runtime/astra-runtime/src/event.rs`、`Engine/Source/Runtime/astra-runtime/src/state_machine.rs`、`Engine/Source/Runtime/astra-runtime/src/world.rs`、`Engine/Source/Runtime/astra-runtime/tests/state_machine_tick.rs`

**Steps:**

1. 定义 EventId、EventPayload、EventQueue 和 deterministic ordering。
2. 实现 StateMachine definition、state、transition、guard、multi-action transition 和 action trace。
3. 让 Scheduler 在固定 tick 边界消费 input、event 和 action result。
4. 输出 TickReport 的 state/event/presentation hash。
5. 编写同 seed、同 input 重跑两次 hash 一致的测试。

**Done Evidence:** `state_machine_tick` 证明 event 顺序稳定、multi-action 按定义顺序执行、单 tick run-to-quiescence 到稳定态/terminal、cycle 和 microstep 超限 blocking，失败时回滚该 machine 的整段候选 mutation。

**Linked Test IDs:** `T-S1-RUNTIME-02`

## S1-RUNTIME-04 StateMachine action provider 与 delayed event

**ID:** `S1-RUNTIME-04`

**Goal:** 让 Stage 1 flat FSM 支撑 deterministic gameplay action provider、host mutation context 和 fixed tick delayed event。

**Depends On:** `S1-RUNTIME-02`、`S1-PLUGIN-01`

**Target Paths:** `Engine/Source/Runtime/astra-runtime/src/action.rs`、`Engine/Source/Runtime/astra-runtime/src/delayed_event.rs`、`Engine/Source/Runtime/astra-runtime/src/state_machine.rs`、`Engine/Source/Runtime/astra-runtime/tests/state_machine_tick.rs`、`Engine/Source/Runtime/astra-runtime/tests/delayed_event.rs`

**Steps:**

1. 把 `TransitionDefinition` 升级为 `actions: Vec<ActionInvocation>`，同一 transition 顺序执行。
2. 实现 `DeterministicActionContext`，只开放 Actor/Component、Blackboard、Event、Await、Presentation 和 delayed event mutation。
3. 实现 `DelayedEventQueue`，按 `(due_tick, sequence, id)` drain，并进入 Runtime snapshot 与 save/replay。
4. 实现 action 失败策略：当前 machine 不迁移，候选 mutation 不提交，blocking diagnostic 进入 TickReport。
5. 暴露 `RuntimeWorld::register_action`、`unregister_action_provider`、`schedule_event` 和 `cancel_delayed_event`。

**Done Evidence:** `state_machine_tick` 覆盖 multi-action 顺序、整机事务回滚、ActionRegistry duplicate/provider conflict 与 descriptor 校验；`trigger_event` 覆盖 typed component mutation；`delayed_event` 覆盖 schedule、cancel、drain order 和 save/load。序列化 effect 会进入 Runtime snapshot 与 presentation hash。

**Linked Test IDs:** `T-S1-RUNTIME-04`

## S1-RUNTIME-03 AwaitToken 与异步结果收敛

**ID:** `S1-RUNTIME-03`

**Goal:** 所有可挂起 action 都落成可序列化 AwaitToken，异步结果在固定 tick 边界有序进入 Runtime。

**Depends On:** `S1-RUNTIME-02`

**Target Paths:** `Engine/Source/Runtime/astra-runtime/src/await_token.rs`、`Engine/Source/Runtime/astra-runtime/src/world.rs`、`Engine/Source/Runtime/astra-runtime/tests/await_token.rs`

**Steps:**

1. 定义 AwaitToken、AwaitKind、AwaitReplayPolicy 和 deterministic timeout。
2. 实现 `start -> await token -> resume` 的 action trace。
3. 让 AwaitResult 按 token id 和 sequence 排序后进入 EventQueue。
4. 保存 await queue 到 Runtime snapshot。
5. 编写异步结果乱序提交但 tick hash 一致的测试。

**Done Evidence:** `await_token` 证明 Runtime deterministic state 不依赖 task completion order；`RecordedResult` 与 `DeterministicTimeout` 的 token 形状、completion 来源和 timeout step 会被校验，非法 live timeout result 阻断。

**Linked Test IDs:** `T-S1-RUNTIME-03`

## S1-SAVE-01 Save/Replay binary container

**ID:** `S1-SAVE-01`

**Goal:** 用自描述二进制容器保存 Runtime state、事件 trace、AwaitToken、DelayedEventQueue 和 migration manifest。

**Depends On:** `S1-RUNTIME-03`、`Docs/contracts/data-formats.md`

**Target Paths:** `Engine/Source/Runtime/astra-runtime/src/save.rs`、`Engine/Source/Runtime/astra-runtime/src/world.rs`、`Engine/Source/Runtime/astra-runtime/tests/save_replay.rs`

**Steps:**

1. 定义 container header、section table、section payload 和 footer hash。
2. 使用 serde/postcard 保存 Runtime、Actor/Component、StateMachine、Blackboard、AwaitToken 和 delayed event queue section。
3. 实现 save、load、replay 和 hash mismatch diagnostic。
4. 加入 SchemaMigrator registry 和 migration chain 校验。
5. 编写 save-load-replay hash 一致和迁移链缺失失败测试。

**Done Evidence:** `save_replay` 覆盖 save/load 后 StableId sequence 连续、future EventQueue pending/trace/next_sequence 不丢失、provider output payload/effect hash 校验、player input/AwaitResult/provider output 的 provider-free replay，以及逐 tick state/event/presentation checkpoint mismatch blocking。

**Linked Test IDs:** `T-S1-SAVE-01`

## S1-PLUGIN-01 Plugin descriptor 与 loader lifecycle

**ID:** `S1-PLUGIN-01`

**Goal:** `astra-plugin` 实现 descriptor validation、fingerprint gate、registry 和 load/unload lifecycle。

**Depends On:** `S1-CORE-01`、`Docs/contracts/plugin-abi.md`

**Target Paths:** `Engine/Source/Runtime/astra-plugin/src/descriptor.rs`、`Engine/Source/Runtime/astra-plugin/src/loader.rs`、`Engine/Source/Runtime/astra-plugin/src/registry.rs`、`Engine/Source/Runtime/astra-plugin/tests/descriptor_gate.rs`、`Engine/Source/Runtime/astra-plugin/tests/load_unload.rs`

**Steps:**

1. 定义 PluginDescriptor、capability、permission、engine version、rustc fingerprint 和 feature fingerprint。
2. 实现 descriptor YAML parsing、`abi_style`、binary hash 和 validation diagnostics。
3. 建立 ServiceRegistry、ExtensionRegistry 和 EngineModuleSlot 注册入口。
4. 用 `abi_stable::RootModule` 和 `libloading` 实现 load/unload 状态机，不支持运行中热重载。
5. 编写 fingerprint mismatch、permission missing 和 unload cleanup 测试。

**Done Evidence:** 插件 gate 能拒绝不匹配 descriptor，fixture cdylib 通过 root module 注册 provider，unload 会清理 registry 并记录 machine-readable diagnostic。

**Linked Test IDs:** `T-S1-PLUGIN-01`

## S1-DYLIB-01 Engine Rust dylib target

**ID:** `S1-DYLIB-01`

**Goal:** `astra-engine` 提供官方 Rust ABI dynamic library facade，复用当前 EngineCore crate，不生成第二套 Runtime。

**Depends On:** `S1-CORE-01`、`S1-RUNTIME-01`、`S1-SAVE-01`、`S1-PLUGIN-01`

**Target Paths:** `Engine/Source/Runtime/astra-engine/`、`Cargo.toml`

**Steps:**

1. 新增 `astra-engine` crate，并声明 `crate-type = ["rlib", "dylib"]`。
2. 通过 `core`、`runtime`、`package` 和 `plugin` module re-export 当前 public API。
3. 文档明确 Rust dylib 只承诺同 engine version、rustc fingerprint 和 feature fingerprint 下动态链接。
4. 编写 facade smoke test，确认可创建 `RuntimeWorld`、构建 package，并访问 plugin registrar。

**Done Evidence:** `cargo test -p astra-engine dylib_facade` 通过；`astra-engine` 不引入 C ABI、COM ABI 或第二套 runtime。

**Linked Test IDs:** `T-S1-DYLIB-01`

## S1-PLUGIN-02 FFI StateMachine action provider

**ID:** `S1-PLUGIN-02`

**Goal:** `astra-plugin` 能把 ABI-safe function pointer action 注册为 Runtime action provider，并在 unload 时清理。

**Depends On:** `S1-PLUGIN-01`、`S1-RUNTIME-04`

**Target Paths:** `Engine/Source/Runtime/astra-plugin/src/abi.rs`、`Engine/Source/Runtime/astra-plugin/src/action_adapter.rs`、`Engine/Source/Runtime/astra-plugin/src/loader.rs`、`Engine/Plugins/Fixtures/headless-presentation-provider/src/lib.rs`、`Engine/Source/Runtime/astra-plugin/tests/ffi_action_provider.rs`

**Steps:**

1. 扩展 `FfiPluginRegistration`，加入 `FfiActionRegistration` 列表。
2. 用 `extern "C" fn(RVec<u8>) -> RVec<u8>` 作为 action invoke 边界，request/result 使用 postcard 编码。
3. 在 host adapter 中把插件返回的 `ActionEffect` 应用到 `DeterministicActionContext`。
4. 扩展 fixture cdylib，注册 `astra.fixture.action.set_flag` 并返回 blackboard、event 和 presentation effect。
5. load/register/execute/unload 全流程覆盖真实 cdylib。

**Done Evidence:** `ffi_action_provider` 测试证明 fixture action 能通过状态机执行，并且 unload 清理 registry。

**Linked Test IDs:** `T-S1-PLUGIN-02`

## S1-PLUGIN-03 Extension registry backend

**ID:** `S1-PLUGIN-03`

**Goal:** 插件后端在 Stage 1 提供 `LoadPhase`、extension registry、dependency graph、explicit binding 和 conflict report，不依赖 Editor UI。

**Depends On:** `S1-PLUGIN-01`、`S1-PLUGIN-02`、`Docs/implementation/provider-plugin-api.md`

**Target Paths:** `Engine/Source/Runtime/astra-plugin-abi/`、`Engine/Source/Runtime/astra-plugin/src/registry.rs`、`Engine/Source/Runtime/astra-plugin/tests/extension_registry.rs`

**Steps:**

1. 新增 `astra-plugin-abi`，承载 ABI-safe FFI structs、`LoadPhase`、provider extension、dependency、conflict 和 report DTO。
2. `astra-plugin` 继续作为 host loader/registry/adapter，并 re-export ABI crate。
3. `PluginRegistrar` 保留显式 provider binding；后加载 provider 只能形成 conflict report，不能抢占已选 provider。
4. Fixture dynamic plugin 依赖 `astra-plugin-abi`，继续覆盖真实 load/unload。
5. 编写 extension registry 测试，覆盖 binding、conflict、dependency graph 和 unload cleanup。

**Done Evidence:** `cargo test -p astra-plugin extension_registry` 通过；`cargo test -p astra-plugin` 继续覆盖 descriptor、load/unload 和 FFI action。

**Linked Test IDs:** `T-S1-PLUGIN-03`

## S1-PROP-01 PropertySystem derive 调试路径

**ID:** `S1-PROP-01`

**Goal:** `astra-property` 提供 PropertySystem metadata 和 derive 宏的可调试路径。

**Depends On:** `S1-CORE-01`

**Target Paths:** `Engine/Source/Developer/astra-property/`、`Engine/Source/Developer/astra-property-derive/`、`Engine/Source/Developer/astra-property/tests/property_metadata.rs`、`Engine/Source/Developer/astra-property/tests/expand_smoke.rs`

**Steps:**

1. 定义 property metadata、schema field、Inspector field 和 save/replay glue metadata。
2. 实现 derive 宏只生成显式字段注册代码，不生成隐藏继承或全局对象系统。
3. 提供 `cargo expand` 可读的宏输出路径。
4. 编写 metadata snapshot 和 expand smoke 测试。

**Done Evidence:** derive 输出可检查，metadata 可供 Inspector/MCP/save glue 消费。

**Linked Test IDs:** `T-S1-PROP-01`

## S1-TEST-01 Headless scenario runner 与 native smoke

**ID:** `S1-TEST-01`

**Goal:** `astra-test` 能运行 YAML scenario，生成 headless report，并覆盖 native smoke。

**Depends On:** `S1-SAVE-01`、`S1-PLUGIN-01`

**Target Paths:** `Engine/Source/Developer/astra-test/src/runner.rs`、`Engine/Source/Developer/astra-test/src/report.rs`、`Engine/Source/Programs/astra-cli/src/main.rs`、`scenarios/native_smoke.yaml`、`Engine/Source/Developer/astra-test/tests/native_smoke.rs`

**Steps:**

1. 定义 `astra.scenario.v1` action、assertion 和 seed 输入。
2. 实现 headless runner，把 launch、advance、save、load、replay 映射到 Runtime API。
3. 输出 scenario report，包含 state/event/presentation hash 和 diagnostics。
4. 创建 native smoke scenario 覆盖 boot、dialogue scripted event、choice、FFI action、delayed event、save/load 和 replay。
5. 编写 CLI smoke 测试和 report schema 测试。

**Done Evidence:** `astra test run scenarios/native_smoke.yaml --headless` 输出 machine-readable report，且 replay hash、FFI action provider 和 delayed event checks 通过。

**Linked Test IDs:** `T-S1-TEST-01`

## S1-OBS-01 Stage 1 结构化日志

**ID:** `S1-OBS-01`

**Goal:** 已实现链路使用 `tracing` 输出可脱敏结构化日志，CLI 保持 report stdout 与日志 stderr/file 分离。

**Depends On:** `S1-RUNTIME-02`、`S1-PLUGIN-01`、`S1-TEST-01`

**Target Paths:** `Cargo.toml`、`Engine/Source/Runtime/astra-runtime/src/world.rs`、`Engine/Source/Runtime/astra-runtime/src/state_machine.rs`、`Engine/Source/Runtime/astra-plugin/src/`、`Engine/Source/Developer/astra-test/src/runner.rs`、`Engine/Source/Programs/astra-cli/src/main.rs`、`Engine/Source/Programs/astra-cli/tests/logging.rs`

**Steps:**

1. 在 workspace 依赖中接入 `tracing`、`tracing-subscriber` 和 `tracing-appender`。
2. 让库 crate 只发 span/event，由 `astra-cli` 统一安装 subscriber。
3. 在 Runtime tick、StateMachine action、plugin load/unload、FFI action、scenario runner 和 CLI report 写入点记录结构化事件。
4. 保持 report 输出到 stdout，日志输出到 stderr，`--log-dir` 只使用调用者显式传入的目录。
5. 用 CLI 集成测试断言 stdout report 可解析、stderr JSON log 包含 runtime/scenario/plugin 事件，且日志不含本地绝对路径。

**Done Evidence:** `cargo test -p astra-cli --test logging` 通过；日志字段只包含 step、hash、schema、status、diagnostic code、provider/action/plugin id 和计数。

**Linked Test IDs:** `T-S1-OBS-01`

## S1-TARGET-01 Target manifest 与 CLI validation

**ID:** `S1-TARGET-01`

**Goal:** `astra-target` 提供 Editor/Game/Program Target schema、manifest validation 和 CLI 入口。

**Depends On:** `S1-BOOT-01`

**Target Paths:** `Engine/Source/Platform/astra-target/`、`Engine/Source/Programs/astra-cli/src/main.rs`、`Engine/Source/Programs/astra-cli/tests/target_platform.rs`

**Steps:**

1. 定义 `TargetKind`、`TargetDescriptor`、`TargetManifest` 和 `TargetValidationReport`。
2. 读取 `project.yaml targets`，兼容旧 project 生成 default Game target。
3. 阻断重复 Target、Game 不可打包、Editor 被打包和缺失选择目标。
4. 暴露 `astra target list` 和 `astra target validate`。
5. 编写 CLI JSON report 测试。

**Done Evidence:** `cargo test -p astra-target` 和 `cargo test -p astra-cli --test target_platform` 通过。

**Linked Test IDs:** `T-S1-TARGET-01`

## 跨 Stage Observability follow-up

`S1-OBS-01` 仍保留为 Stage 1 CLI `tracing` 基线。当前生产级升级由 `OBS-CORE-01`、`OBS-COVERAGE-01` 和 `OBS-CRASH-WIN-01` 跟踪，覆盖共享 sink、workspace 运行链路和 Windows 进程外 crash reporter；证据见 [implementation plan](../implementation-plan.md) 与 [logging coverage](../logging-coverage.md)。
