# ADR 0012: AstraEMU engine-native family plugin

## Context

AstraEMU 需要复用 AstraEngine Runtime、Media、Plugin、Save/Replay 和 Release Gate。旧方案把 family core 放在独立边界，能隔离崩溃，但会形成第二套 tick、snapshot、report 和 provider 管线，AstraEngine 的 StateMachine、插件诊断和 package/save 容器无法成为唯一验收路径。

后续 family 研究又暴露出另一个问题：如果把 archive、script、action、media 和 snapshot 拆成多个顶层 provider，Manager 会被迫理解旧引擎内部阶段。SoftPAL 的 extcall、FVP 的 multi-context VM、Siglus 的 form/element dispatch 都不适合作为 EngineCore public API。

## Decision

AstraEMU v1 采用 Manager + AstraEngine `RuntimeWorld` + in-process family plugin。Manager 负责窗口、输入、配置、插件启用和报告；`RuntimeWorld` 持有 tick、MutationLog、Save/Replay 和 deterministic replay；family plugin 通过 ExtensionRegistry 注册一个 `LegacyRuntimeProvider` facade。

`LegacyRuntimeProvider` 是 family runtime 的唯一主入口。它用 `probe`、`open`、`step`、`save`、`restore` 和 `shutdown` 管理 session，`open` 返回 `LegacyRuntimeSessionId`。family VM、archive resolver、script executor、media state 和 snapshot serializer 都留在 session 内。AstraEngine StateMachine 只驱动 `Booting`、`Active`、`Awaiting`、`Saving`、`Loading`、`Faulted`、`Shutdown` 这类粗粒度生命周期。

`EMUCoreBridge` 保留为普通 extension point，用于外部工具桥接和研究环境。它不属于 v1 主架构，也不能替换 `RuntimeWorld`、MutationLog、Save container 或 Release Gate core checks。

## Consequences

- 旧引擎语义必须落成可序列化 `LegacyEffect`、RuntimeEvent、PresentationCommand、AudioCommand、TextCaptureEvent、AwaitToken 和 package/save section。
- family plugin 不能替换 Runtime tick、MutationLog、Save container、Release Gate core checks、renderer/audio native handle 或 StateMachine 调度。
- 插件权限、dependency graph、enablement、redaction 和 packaged 裁剪都走统一 Plugin Manager 和 Release Gate。
- 崩溃隔离改由 capability sandbox、permission policy、deterministic effect list、redacted report、provider unload 和 headless scenario gate 共同处理。
- Artemis 仍是 v1 可用 family；KrKr、BGI、SoftPAL、FVP、Siglus 和 Minori 先输出 alpha probe report，再补 full-flow gate。

## Verification

```bash
cargo test -p astra-emu-manager manager_runtime_world
cargo test -p astra-emu-family-api legacy_runtime_provider_api
astra test run scenarios/emu/artemis_full_flow.yaml --headless --report target/reports/artemis.yaml
cargo test -p astra-release emu_gate
```

Expected report includes `emu.legacy_runtime_provider`, `plugin.extension_registry`, `runtime.replay.determinism`, `emu.artemis_full_flow` and `emu.report_redaction`.
