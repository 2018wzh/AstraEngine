# Game Runtime Provider Blueprint

本页描述 [Game Runtime Provider Contract](../contracts/game-runtime-provider.md) 的实现落点。目标是把“玩法类型”变成可替换 runtime provider，而不是让某个垂直模块成为所有玩法的父类。

## Selection

Project target 通过 manifest 显式选择 runtime provider：

```yaml
targets:
  nativevn-game:
    kind: game
    runtime_provider: native_vn
    profiles: [classic, modern]
  emu-case:
    kind: game
    runtime_provider: astra_emu
    profiles: [classic]
```

Provider selection 读取 extension registry 和 provider policy。缺 provider、provider fingerprint 不匹配、capability 不足、package section 缺失或 profile 不允许时，release gate 和 runtime launch 都必须阻断。Editor 可以显示可选 provider，但不能绕过 manifest binding。

Package evidence 复用 `provider.policy`，不新增 `runtime.provider_manifest` section。Plugin loader 读取 `FfiPluginRegistration.runtime_providers` 后，仍把 runtime provider 写入现有 provider registry snapshot；release gate 用 `provider.policy` 的 selected runtime provider descriptor/binding、`plugin.extension_registry` 的 `game_runtime_provider` slot 和 target manifest 的 `runtime_provider` 三方交叉校验。

## RuntimeWorld Integration

`RuntimeWorld` 不直接知道 VN、EMU 或 RPG。Game runtime provider 通过一个 StateMachine action bridge 被调用：

```text
RuntimeWorld tick
  -> GameRuntimeStep action
  -> ProductRuntimeProvider::step
  -> RuntimeStepOutput
  -> host adapter applies effects
  -> AwaitToken and delayed events return on a fixed tick
```

Provider 输出只能是可序列化 effect list、await token、presentation/audio command、diagnostic、trace 和 dirty save section。Host adapter 负责用 `DeterministicActionContext` 提交变更。Provider action 失败时，不提交候选 mutation，当前 machine 进入 release profile 指定的 fault policy。

## NativeVN Provider

`NativeVnRuntimeProvider` 已位于 `astra-vn-runtime-provider`，包装 AstraVN 功能 crate：

- `prepare` 编译 `.astra`、policy bundle、system story、command manifest 和 presentation profile。
- `probe` 校验 package sections、target/profile、scenario refs 和 player route model。
- `open` 创建 session-owned `RuntimeWorld`、VN Actor、typed VN/policy components、runtime cursor、policy state 和 flat story StateMachine。
- `step` 把 launch、advance、choose、system page、wait completion 等输入编码成 RuntimeEvent，由 `astra.vn.step` action 推进 dialogue、choice、system story、wait、presentation、audio、timeline 和 mutation。
- `save/restore` 只读写权威 `runtime.world`/`astra.runtime.save_blob.v3` section。Nested Runtime save container 保存完整 RuntimeSnapshot；restore 在 outer hash、container footer、section hash 和 schema/version 全部通过后事务替换 world，并回报 restored step/seed。v2 不提供迁移入口。

Runtime v3 的 Action descriptor 强制包含 execution class、read/write set 和 StableId reservation。FFI Action ABI v2 传递完整 descriptor JSON；metadata 不一致、pure action 声明写入、StableId 声明不闭合、effect 越权或实际 ID 消耗超额都会产生 blocking diagnostic。AstraVN reducer 按实际写入记录 variable mutation journal；backlog、read-state、route coverage 和 voice replay 使用固定 64 条 chunk、stable ordinal/bitset 与历史 root，普通 step 只替换 hot state 和变化的尾 chunk，save/checkpoint 才物化完整 v3 state。Runtime tick 的 Actor、Blackboard、Event、Await 和 DelayedEvent 已切换 inverse journal，conflict-DAG 在明确的 1/2/4/8 worker 配置下并行无冲突 machine。
- `package_sections` 继续输出 `vn.*` sections。
- `release_checks` 继续声明 `vn.commercial_baseline`、`vn.system_ui_profile`、`vn.advanced_presentation`、`player.full_playable` 等 check。

VN Core 保持 dialogue、choice、backlog、save/load、read-state 和 voice replay 的权威语义。Luau policy 和 plugin command 只扩展表现、系统页和高级演出策略。

当前 FFI adapter 有显式 provider instance registry。`create_instance`、`destroy_instance`、`open`、`step`、`save`、`restore` 和 `shutdown` 都调用同一真实 provider 路径；in-process provider 也必须实现真实 instance lifecycle，host 不提供默认成功实现。Host 在 create/open 部分失败时 rollback，校验 instance/session identity、连续 fixed step、1..=1 秒 delta、session seed、live/restore mode、output/save section schema/hash/bounds，并阻断 live provider replay。Restore report 回传 snapshot step/seed，下一 tick 只能使用一次 `RestoreContinuation`，随后恢复 `Live`。Provider panic、错误、malformed output 和 timeout 会进入 poisoned lifecycle。timeout 返回前先等待 blocking worker drain；调用方随后用 `cleanup_after_failure` shutdown 全部 session 并 destroy instance。`open` 从请求中的 `vn.compiled_story` section 解码 story，不能创建未绑定 session。外部 dylib 的分发、签名和版本协商仍留给插件发布工作，不影响当前 ABI lifecycle 行为证据。

Release validator 从 package 内的 `vn.compiled_story` 执行 package-bound lifecycle conformance，并记录 state/event/presentation hash。Runtime replay 另存 hash-validated `ProviderReplayOutput`，回放阶段不调用 FFI 或 in-process provider。

## Concurrent Session Migration

Runtime 已新增 `ProductRuntimeProviderFactory`、`ProductRuntimeSession` 和 `ConcurrentProductRuntimeHost`。Factory 只持有 instance control state；`open` 返回独占 session object，每条 session 使用容量 32 的 ordered mailbox、fixed-step authority 和 poison state，不再经过全局 `ProductRuntimeHost` mutex。不同 session 可以同时进入有界 worker，同一 session 的 `step/save/restore/shutdown` 严格 FIFO、单飞。`NativeVnRuntimeProviderFactory` 与 `AstraEmuRuntimeProviderFactory` 已把每条 session 隔离到独立 `RuntimeWorld`。

`astra-headless run-batch` 从 `astra.headless_session_batch.v2` manifest 启动最多八个独立 `astra-headless run` 子进程。`worker_limit` 是允许使用的全局上限，runner 再按 job 数量与 `available_parallelism` 自动选择实际并发度，不能用高于硬件或任务数量的空额度美化利用率。串行 baseline 的单个 Session 可独占完整预算；并发阶段每个子 Session 获得 `floor(global_limit / selected_concurrency)` 个内部 worker，因此所有同时运行子进程的内部配额总和不会超过全局上限，也不会让 Session 内 worker pool 在父级并发之外再次超订阅。每条 job 声明 `route`、`replay` 或 `performance` 类型，以及独立 concurrent/serial artifact root、timeout、package/input/profile/build identity。只有 `performance` job 必须声明 performance budget、warmup 和 measurement start；route/replay job 若夹带 performance 配置会 fail closed。Runner 先生成同身份串行 baseline，再执行公平排队的 concurrent batch；报告 `astra.headless_session_batch_report.v2` 按 session id 稳定排序，记录 output identity 对比、配置上限、硬件并行度、实际并发度、串行/并发每 Session 配额、并发总容量、排队/执行时间、批次 wall time、吞吐、Session slot utilization、按子进程 kernel+user CPU time 与全局容量归一化的 worker utilization、所有 job 的串行/并行 private-memory peak 和串行 baseline；performance job 另记录每 session CPU/E2E p95/p99。Windows runner 直接按子进程 PID 采样 private bytes 与 CPU time，并把 private bytes 与 performance report 内的峰值取较大值；任一采样不受支持、失败或返回空值时 blocking，不写 `null` 冒充完成。任一 session 失败或超时不会取消已开始任务，但 identity mismatch 或任一失败会使批次最终 blocking。

旧同步 `ProductRuntimeHost` 暂时只保留给仍需迁移的 dynamic FFI/shipping host 调用点，不代表 Provider ABI v1 仍受支持；ABI DTO、save 与 action contract 已硬切 v2/v3。删除同步 facade 仍需同身份 Headless batch 与 Windows Player evidence，因此当前状态保持 `IN_PROGRESS`。

## Presentation 与资源流水线

`ProductStageDirector` 持有序列化 `PresentationCoordinator`，其 Character、Background、Text、Video region 从同一 snapshot 产生 delta并按稳定 sequence 合并。命令必须显式声明 `queue`、`replace_from_current`、`snap_then_start` 或 `reject`；跨 region layer 写冲突、非法 queue activation 和 fence drift fail closed。TextRegion 拥有 layout/reveal/auto timer/window 逻辑，第一次点击只完成 reveal；VideoRegion 记录 Prebuffering/Playing/Ended/Failed 和逻辑播放点，设备 decoder/GPU handle 不进入 snapshot。

CosmicText shaping 使用 worker-local `FontSystem`/`SwashCache`、分片 single-flight cache；图片预取、audio/video decode 和 region preparation 从同一 `WorkerBudgetBroker` 租赁额度。静态 RGBA 在 `UploadTexture` command 取得 `Arc` 后立即从 CPU asset cache 释放，保留 manifest、hash 和尺寸；device loss 清空 texture/glyph residency与 retained draw cache，下一帧从 package manifest 重建。

## AstraEMU Provider

`AstraEmuRuntimeProvider` 是 AstraEMU 的 gameplay runtime facade。Manager 仍是 Program target，可以负责窗口、输入、profile、overlay、文本管线和 UI；被启动的 legacy case 作为 Game target runtime session 运行。

`AstraEmuRuntimeProvider` 内部继续选择 family `LegacyRuntimeProvider`。Family provider 持有旧 VM、pack resolver、media bridge 和 snapshot serializer；它不能替换 `RuntimeWorld`、MutationLog、Save container 或 Release Gate。EMU provider 把 family step 输出转换成 Runtime effect list、AwaitToken、PresentationCommand、AudioCommand、TextCaptureEvent、snapshot section 和 local case report。

## AstraRPG Provider

`AstraRpgRuntimeProvider` 是后续同级 runtime。设计只预留同一 provider boundary：map、party、battle、inventory、quest、encounter、AI behavior、committed output 和 RPG editor metadata 都通过 provider package sections、runtime effects、save sections 和 release checks 接入。TRPG 玩法落在 AstraRPG 的 `rpg.trpg` profile/ruleset layer；不创建独立 `AstraTrpgRuntimeProvider`，也不使用顶层 `trpg.*` section。当前仓库不把 AstraRPG 写成已有实现，也不把 VN Core 抽成 RPG base class。

## Migration Rule

已有 AstraVN facade、VN extension manifest、package sections 和 release checks 先按 module layout 与 crate split 迁移，再由 `astra-vn-runtime-provider` 组合为 `NativeVnRuntimeProvider`。已有 plugin registry/action provider/VN extension fixture 迁移到 provider selection 口径。AstraEMU/AstraRPG 尚无实现代码，迁移文档只写未来建设计划，不列为现有代码搬迁；AstraRPG 的前置迁移见 [AstraRPG Design Alignment Migration](../migrations/astra-rpg-design-alignment-migration.md)。
