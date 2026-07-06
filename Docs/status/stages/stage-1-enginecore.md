# Stage 1 EngineCore Work

Stage 1 已交付 EngineCore 的可运行闭环：UE 风格 workspace、核心类型、Runtime tick、StateMachine、AwaitToken、Save/Replay、Plugin ABI、PropertySystem 和 headless scenario runner。后续 Stage 的页面仍记录未实现工作。

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

**Done Evidence:** RuntimeWorld 可创建空 world，可创建 actor/component，并能输出稳定 snapshot hash。

**Linked Test IDs:** `T-S1-RUNTIME-01`

## S1-RUNTIME-02 StateMachine、EventBus 与 fixed tick

**ID:** `S1-RUNTIME-02`

**Goal:** 实现同步 guard、ordered EventBus、fixed tick Scheduler 和可追踪 StateMachine。

**Depends On:** `S1-RUNTIME-01`

**Target Paths:** `Engine/Source/Runtime/astra-runtime/src/event.rs`、`Engine/Source/Runtime/astra-runtime/src/state_machine.rs`、`Engine/Source/Runtime/astra-runtime/src/world.rs`、`Engine/Source/Runtime/astra-runtime/tests/state_machine_tick.rs`

**Steps:**

1. 定义 EventId、EventPayload、EventQueue 和 deterministic ordering。
2. 实现 StateMachine definition、state、transition、guard 和 action trace。
3. 让 Scheduler 在固定 tick 边界消费 input、event 和 action result。
4. 输出 TickReport 的 state/event/presentation hash。
5. 编写同 seed、同 input 重跑两次 hash 一致的测试。

**Done Evidence:** fixed tick 测试证明 event 顺序不受提交顺序外的因素影响。

**Linked Test IDs:** `T-S1-RUNTIME-02`

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

**Done Evidence:** AwaitToken 测试证明 Runtime deterministic state 不依赖 task completion order。

**Linked Test IDs:** `T-S1-RUNTIME-03`

## S1-SAVE-01 Save/Replay binary container

**ID:** `S1-SAVE-01`

**Goal:** 用自描述二进制容器保存 Runtime state、事件 trace、AwaitToken 和 migration manifest。

**Depends On:** `S1-RUNTIME-03`、`Docs/contracts/data-formats.md`

**Target Paths:** `Engine/Source/Runtime/astra-runtime/src/save.rs`、`Engine/Source/Runtime/astra-runtime/src/world.rs`、`Engine/Source/Runtime/astra-runtime/tests/save_replay.rs`

**Steps:**

1. 定义 container header、section table、section payload 和 footer hash。
2. 使用 serde/postcard 保存 Runtime、Actor/Component、StateMachine、Blackboard 和 AwaitToken section。
3. 实现 save、load、replay 和 hash mismatch diagnostic。
4. 加入 SchemaMigrator registry 和 migration chain 校验。
5. 编写 save-load-replay hash 一致和迁移链缺失失败测试。

**Done Evidence:** save/load/replay 测试能定位 frame、event、actor/component 或 command 维度的 mismatch。

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
4. 创建 native smoke scenario 覆盖 boot、dialogue scripted event、choice、save/load 和 replay。
5. 编写 CLI smoke 测试和 report schema 测试。

**Done Evidence:** `astra test run scenarios/native_smoke.yaml --headless` 输出 machine-readable report，且 replay hash 一致。

**Linked Test IDs:** `T-S1-TEST-01`
