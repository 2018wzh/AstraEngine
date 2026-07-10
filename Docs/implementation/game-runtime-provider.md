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
- `save/restore` 读写并校验 `vn.runtime_state` 与 `vn.policy_state` 的 schema、codec、version、hash 和 postcard payload，restore 后必须复现保存时的状态 hash。
- `package_sections` 继续输出 `vn.*` sections。
- `release_checks` 继续声明 `vn.commercial_baseline`、`vn.system_ui_profile`、`vn.advanced_presentation`、`player.full_playable` 等 check。

VN Core 保持 dialogue、choice、backlog、save/load、read-state 和 voice replay 的权威语义。Luau policy 和 plugin command 只扩展表现、系统页和高级演出策略。

当前 FFI adapter 有显式 provider instance registry。`create_instance`、`destroy_instance`、`open`、`step`、`save`、`restore` 和 `shutdown` 都调用同一真实 provider 路径；`open` 从请求中的 `vn.compiled_story` section 解码 story，不能创建未绑定 session。外部 dylib 的分发、签名和版本协商仍留给插件发布工作，不影响当前 ABI lifecycle 行为证据。

Release validator 从 package 内的 `vn.compiled_story` 执行 package-bound lifecycle conformance，并记录 state/event/presentation hash。Runtime replay 另存 hash-validated `ProviderReplayOutput`，回放阶段不调用 FFI 或 in-process provider。

## AstraEMU Provider

`AstraEmuRuntimeProvider` 是 AstraEMU 的 gameplay runtime facade。Manager 仍是 Program target，可以负责窗口、输入、profile、overlay、文本管线和 UI；被启动的 legacy case 作为 Game target runtime session 运行。

`AstraEmuRuntimeProvider` 内部继续选择 family `LegacyRuntimeProvider`。Family provider 持有旧 VM、pack resolver、media bridge 和 snapshot serializer；它不能替换 `RuntimeWorld`、MutationLog、Save container 或 Release Gate。EMU provider 把 family step 输出转换成 Runtime effect list、AwaitToken、PresentationCommand、AudioCommand、TextCaptureEvent、snapshot section 和 local case report。

## AstraRPG Provider

`AstraRpgRuntimeProvider` 是后续同级 runtime。设计只预留同一 provider boundary：map、party、battle、inventory、quest、encounter、AI behavior、committed output 和 RPG editor metadata 都通过 provider package sections、runtime effects、save sections 和 release checks 接入。TRPG 玩法落在 AstraRPG 的 `rpg.trpg` profile/ruleset layer；不创建独立 `AstraTrpgRuntimeProvider`，也不使用顶层 `trpg.*` section。当前仓库不把 AstraRPG 写成已有实现，也不把 VN Core 抽成 RPG base class。

## Migration Rule

已有 AstraVN facade、VN extension manifest、package sections 和 release checks 先按 module layout 与 crate split 迁移，再由 `astra-vn-runtime-provider` 组合为 `NativeVnRuntimeProvider`。已有 plugin registry/action provider/VN extension fixture 迁移到 provider selection 口径。AstraEMU/AstraRPG 尚无实现代码，迁移文档只写未来建设计划，不列为现有代码搬迁；AstraRPG 的前置迁移见 [AstraRPG Design Alignment Migration](../migrations/astra-rpg-design-alignment-migration.md)。
