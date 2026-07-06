# ADR 0012: AstraEMU engine-native family plugin

## Context

AstraEMU 需要最大化复用 AstraEngine Runtime、Media、Plugin、Save/Replay 和 Release Gate。旧方案把 compat core 放在独立边界，能隔离崩溃，但会形成第二套 tick、snapshot、report 和 provider 管线，AstraEngine 的状态机、插件诊断和 package/save 容器无法成为唯一验收路径。

## Decision

AstraEMU v1 采用 Manager + AstraEngine `RuntimeWorld` + in-process family plugin。Manager 负责窗口、输入、配置、插件启用和报告；RuntimeWorld 持有 tick、MutationLog、Save/Replay 和 deterministic replay；family plugin 通过 ExtensionRegistry 注册 VFS/archive provider、legacy script provider、StateMachine action provider、legacy VM adapter、media mapper、snapshot codec 和 release check。

`EMUCoreBridge` 降级为普通 extension point。它可以服务外部工具桥接和研究环境，但不作为 v1 主架构。

## Consequences

- 旧引擎语义必须落成可序列化 action effect、RuntimeEvent、PresentationCommand、AudioCommand、TextCaptureEvent 和 package/save section。
- family plugin 不能替换 Runtime tick、MutationLog、Save container 或 Release Gate core checks。
- 插件权限、dependency graph、enablement、redaction 和 packaged 裁剪都走统一 Plugin Manager 和 Release Gate。
- 崩溃隔离改由 capability sandbox、permission policy、deterministic effect list、redacted report、provider unload 和 headless scenario gate 共同处理。
- Artemis 仍是 v1 可用 family；KrKr、BGI、SoftPAL、FVP、Siglus 和 Minori 先输出 alpha probe report，再补 full-flow gate。

## Verification

```bash
cargo test -p astra-emu-manager manager_runtime_world
cargo test -p astra-emu-family-api family_plugin_api
astra test run scenarios/emu/artemis_full_flow.yaml --headless --report target/reports/artemis.yaml
cargo test -p astra-release emu_gate
```

Expected report includes `emu.engine_native_family`, `plugin.extension_registry`, `runtime.replay.determinism`, `emu.artemis_full_flow` and `emu.report_redaction`.
