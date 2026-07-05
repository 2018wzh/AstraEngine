# Stage 3 AstraVN Work

Stage 3 把 EngineCore、Media 和 Package 组合成原生 VN 工作流。`.astra` 仍是 canonical story source；AstraVN Core 持有权威语义，Luau policy 只处理表现、系统页和复杂演出。本页是 planned target 清单，不表示实现已经存在。

## S3-SCRIPT-01 `.astra` parser

**ID:** `S3-SCRIPT-01`

**Goal:** 解析 `.astra` 缩进块、story、state、scene、stage、text、choice、call/return 和 command id。

**Depends On:** `Docs/modules/astra-vn-script.md`

**Target Paths:** `crates/astra-vn/src/parser.rs`、`crates/astra-vn/tests/parser_astra.rs`、`samples/nativevn/main.astra` planned target

**Steps:**

1. 定义 lexer、indent block parser、source span 和 parse diagnostic。
2. 支持 command id、text key、speaker、voice、choice option 和 jump target。
3. 保留 source map 所需的 byte range、line/column 和 expanded command id。
4. 编写有效 sample、缩进错误、重复 command id 和缺失 jump target 测试。

**Done Evidence:** parser 能把样例 `.astra` 转成 AST，并对语法错误给出 source span。

**Linked Test IDs:** `T-S3-SCRIPT-01`

## S3-SCRIPT-02 Compiler 到 CompiledStory IR

**ID:** `S3-SCRIPT-02`

**Goal:** 编译 AST 到 CompiledStory IR、StoryManifest、VariableManifest、CommandManifest、SourceMap 和 DebugSymbols。

**Depends On:** `S3-SCRIPT-01`

**Target Paths:** `crates/astra-vn/src/compiler.rs`、`crates/astra-vn/src/compiled_story.rs`、`crates/astra-vn/tests/compiled_story.rs` planned target

**Steps:**

1. 定义 CompiledStory 结构，与 script contract 字段保持一致。
2. 实现 command id 稳定排序、route graph、text key manifest 和 debug symbol 输出。
3. 校验 unreachable state、重复 key、非法变量域和未定义 system story。
4. 编写 IR snapshot、source map lookup 和 reachability diagnostic 测试。

**Done Evidence:** 同一 source 编译两次 IR hash 一致，diagnostic 可定位源文件。

**Linked Test IDs:** `T-S3-SCRIPT-02`

## S3-CORE-01 VN Core dialogue、choice 与变量域

**ID:** `S3-CORE-01`

**Goal:** AstraVN Core 实现 dialogue、choice、variables、call/return 和 route flags 的权威语义。

**Depends On:** `S1-RUNTIME-02`、`S3-SCRIPT-02`

**Target Paths:** `crates/astra-vn/src/runtime.rs`、`crates/astra-vn/src/variables.rs`、`crates/astra-vn/tests/vn_core_flow.rs` planned target

**Steps:**

1. 把 CompiledStory command 驱动为 Runtime StateMachine action。
2. 实现 project、global、temp、system 四个变量域和写入规则。
3. 实现 dialogue wait、choice wait、call/return stack 和 route flag。
4. 编写 dialogue advance、choice branch、variable rollback 和 call/return 测试。

**Done Evidence:** VN Core 测试证明剧情权威状态不依赖 Luau policy。

**Linked Test IDs:** `T-S3-CORE-01`

## S3-CORE-02 Backlog、read-state 与 voice replay

**ID:** `S3-CORE-02`

**Goal:** Backlog、read-state 和 voice replay 由 AstraVN Core 统一维护。

**Depends On:** `S3-CORE-01`、`S2-MEDIA-02`

**Target Paths:** `crates/astra-vn/src/backlog.rs`、`crates/astra-vn/src/read_state.rs`、`crates/astra-vn/src/voice_replay.rs`、`crates/astra-vn/tests/backlog_read_voice.rs` planned target

**Steps:**

1. 定义 BacklogEntry，保存 text key、speaker、voice ref、layout metadata 和 route position。
2. 实现 read-state mark、skip eligibility 和 voice replay lookup。
3. 确认 Luau policy 只能请求展示，不能改写 Core backlog/read-state。
4. 编写 backlog append、skip read-only、voice replay available 和 replay hash 测试。

**Done Evidence:** Backlog 和 read-state 随 save/load/replay 保持一致。

**Linked Test IDs:** `T-S3-CORE-02`

## S3-CORE-03 VN save/load/replay integration

**ID:** `S3-CORE-03`

**Goal:** VN 状态接入 Stage 1 Save/Replay，覆盖 route、变量、backlog、read-state、voice replay 和 Luau snapshot ref。

**Depends On:** `S1-SAVE-01`、`S3-CORE-02`

**Target Paths:** `crates/astra-vn/src/save.rs`、`crates/astra-vn/tests/vn_save_replay.rs` planned target

**Steps:**

1. 定义 VN save section，包含 route state、command cursor、variables、backlog、read-state 和 voice replay index。
2. 接入 Runtime replay hash，输出 VN command 维度 mismatch。
3. 处理 Luau policy snapshot ref，但不保存 function、thread、userdata 或 native handle。
4. 编写 save-load-resume、replay-from-start 和 invalid snapshot 测试。

**Done Evidence:** VN save/load/replay 测试可证明同输入同 seed 下 hash 一致。

**Linked Test IDs:** `T-S3-CORE-03`

## S3-LUAU-01 Luau sandbox 与 Mutation API

**ID:** `S3-LUAU-01`

**Goal:** Luau 通过 `mlua` 进入策略层，默认无文件、网络或系统调用，权威写入必须走 `astra.mutate`。

**Depends On:** `S3-CORE-01`、`Docs/contracts/script-vn.md`

**Target Paths:** `crates/astra-vn/src/luau_policy.rs`、`crates/astra-vn/src/mutation.rs`、`crates/astra-vn/tests/luau_sandbox.rs` planned target

**Steps:**

1. 建立 Luau runtime sandbox，默认禁用 fs、network 和系统调用。
2. 实现 `astra.command`、`astra.mutate`、`astra.var`、`astra.query` 和 `astra.trace` public API。
3. 记录每次 mutation 的 trace、rollback scope 和 replay event。
4. 拒绝不可序列化 snapshot value。
5. 编写 sandbox denied、mutation recorded、direct table write ignored 和 snapshot blocked 测试。

**Done Evidence:** Luau policy 不能绕过 Core save/backlog/read-state 语义。

**Linked Test IDs:** `T-S3-LUAU-01`

## S3-LUAU-02 官方 policy bundle 与 system stories

**ID:** `S3-LUAU-02`

**Goal:** 提供官方标准 policy bundle，覆盖 message UI、choice UI、title、config、gallery、replay 和 chart system stories。

**Depends On:** `S3-LUAU-01`

**Target Paths:** `samples/nativevn/policies/standard_policy.luau`、`samples/nativevn/system.astra`、`crates/astra-vn/tests/system_stories.rs` planned target

**Steps:**

1. 定义 `astra.policy_bundle.v1` manifest、Luau entry、capabilities、dependencies 和 package lock。
2. 实现 title、config、gallery、replay、chart 入口声明和缺失检查。
3. 让 policy command 提供 schema、Editor metadata、performance budget 和 release check。
4. 编写 system story reachability、missing entry 和 policy lock 测试。

**Done Evidence:** 标准 policy bundle 可以随 package 固定，release gate 能检查 lock/vendor cache。

**Linked Test IDs:** `T-S3-LUAU-02`

## S3-EDIT-01 Graph/Timeline 同源 metadata

**ID:** `S3-EDIT-01`

**Goal:** Graph 和 Timeline 只保存作者 metadata，必须能回写或编译到同一 IR、source map 和 debug symbol。

**Depends On:** `S3-SCRIPT-02`

**Target Paths:** `crates/astra-vn/src/editor_metadata.rs`、`crates/astra-vn/tests/graph_timeline_roundtrip.rs` planned target

**Steps:**

1. 定义 Graph node metadata、Timeline track metadata 和 command id binding。
2. 实现 metadata -> `.astra` patch 或 policy override 的稳定回写路径。
3. 校验 metadata 不产生第二套 runtime model。
4. 编写 graph roundtrip、timeline fence 和 source map identity 测试。

**Done Evidence:** Graph/Timeline 修改后仍指向同一 command id 和 source map。

**Linked Test IDs:** `T-S3-EDIT-01`

## S3-SAMPLE-01 Commercial baseline sample 与 full playthrough

**ID:** `S3-SAMPLE-01`

**Goal:** 建立 NativeVN commercial baseline sample 和 full playthrough scenario。

**Depends On:** `S3-CORE-03`、`S3-LUAU-02`、`S2-GATE-01`

**Target Paths:** `samples/nativevn/`、`scenarios/full_playthrough.yaml`、`crates/astra-test/tests/vn_full_playthrough.rs` planned target

**Steps:**

1. 创建 sample project，覆盖 dialogue、choice、backlog、auto、skip、read-state、config、gallery、replay、voice replay、movie 和 transition。
2. Cook 并 package sample，记录 package id、profile 和 scenario refs。
3. 编写 full playthrough scenario，覆盖启动、路线、系统页、save/load 和 replay_from_start。
4. 接入 release gate，验证 Luau policy、localization、timeline fence 和 replay hash。

**Done Evidence:** `astra test run scenarios/full_playthrough.yaml --package target/nativevn.astrapkg --headless` 通过，并输出 release report。

**Linked Test IDs:** `T-S3-SAMPLE-01`
