# Save / Replay Production Contract

状态：Production contract draft / not yet fully implemented  
定位：定义 save container、section manifest、migration、replay stream 和 mismatch localization 的生产契约。本文补足 `runtime-core.md` 与 `script-and-presentation.md` 中“要保存什么”的接口级细节。

## 1. 目标

- Save 保存确定性运行时状态，不保存 native pointer、线程、GPU/audio handle、Editor widget 或 ECS entity。
- Replay 使用 package manifest、runtime events、script decisions、input records 和 committed outputs，不重新请求外部 provider。
- Migration 覆盖 runtime snapshot、component schema、module extension state、script state 和 legacy extension state。
- Replay mismatch 可定位到 frame、event、actor、component、script command、presentation command 或 provider output hash。

非目标：

- Save 不保存 DDC、Cooked artifact 原始缓存或 Editor-only preview state。
- Replay 不重新执行 AI provider、Editor draft generation 或 non-deterministic network call。

## 2. Save Container

容器头：

```yaml
schema: astra.runtime.save_container.v2
engine_version: 0.2.0
project_id: native.project.sample
project_version: 1
package_manifest_hash: "..."
runtime_profile: deterministic
created_frame: 120
sections: []
integrity:
  hash: "..."
  compression: zstd
```

Section descriptor：

```yaml
schema: astra.runtime.save_section.v1
section_id: section:/runtime/world
owner_module: astra.runtime
payload_schema: astra.runtime.world_snapshot.v2
payload_version: 2
required: true
hash: "..."
compression: zstd
recovery_policy: fail_load
```

Required production sections：

- `section:/runtime/world`
- `section:/scene/actors`
- `section:/runtime/event_bus`
- `section:/runtime/scheduler`
- `section:/runtime/director`
- `section:/script/runtime`
- `section:/presentation/state`
- `section:/media/logical_state`
- `section:/modules/extension_state`
- `section:/ai/committed_output`

## 3. Save Provider Interface

准接口：

```cpp
struct SaveSectionDescriptor;
struct SaveWriteContext;
struct SaveReadContext;

class ISaveSectionProvider {
public:
    virtual std::vector<SaveSectionDescriptor> DescribeSections() const = 0;
    virtual Result<ByteBuffer> WriteSection(SaveWriteContext context, DiagnosticSink& diagnostics) = 0;
    virtual Result<void> ReadSection(SaveReadContext context, DiagnosticSink& diagnostics) = 0;
};
```

规则：

- Module extension state 必须通过 `ISaveSectionProvider` 注册，不能把私有 state 塞进 Core schema。
- Section owner module 的 ABI version 和 payload schema version 必须进入 container header。
- `required=false` section 允许 partial recovery，但必须生成 warning diagnostic 和 replay compatibility note。

## 4. Migration

Migration edge：

```yaml
schema: astra.runtime.save_migration_edge.v1
payload_schema: astra.vn.session.snapshot
from_version: 1
to_version: 2
owner_module: astra.vn
unknown_field_policy: preserve
diagnostic_prefix: ASTRA_SAVE_VN
```

规则：

- Release Gate 必须验证 package 目标 profile 支持从声明的 minimum save version 迁移到当前版本。
- Component migration 与 PropertySystem schema migration 共用 diagnostic format。
- 缺失 required section migration 是 blocking diagnostic。
- Legacy save extension state 只能在 compatibility expansion profile 中启用。

## 5. Replay Stream

Replay stream：

```yaml
schema: astra.runtime.replay_stream.v1
package_manifest_hash: "..."
initial_save_hash: "..."
records:
  - frame: 120
    sequence: 440
    kind: runtime_event
    hash: "..."
checkpoints:
  - frame: 120
    state_hash: "..."
    event_hash: "..."
    presentation_hash: "..."
```

Record kinds：

- `input`
- `runtime_event`
- `script_decision`
- `choice_selection`
- `scheduler_wake`
- `director_decision`
- `presentation_hash`
- `media_provider_hash`
- `committed_ai_output`

## 6. Mismatch Localization

Mismatch report：

```yaml
schema: astra.runtime.replay_mismatch.v1
frame: 120
category: presentation
expected_hash: "..."
actual_hash: "..."
nearest_event_sequence: 440
objects:
  - kind: actor
    id: actor:/systems/dialogue
source:
  file: Content/Scripts/opening.astra
  line: 42
```

规则：

- Replay compare 先定位 checkpoint，再定位 record，再映射 source location。
- Provider hash mismatch 必须包含 selected provider id 和 provider feature set hash。
- Script mismatch 必须包含 source map id、command id、runtime label/node id。

## 7. Diagnostics And Acceptance

诊断前缀：

- `ASTRA_SAVE_*`
- `ASTRA_REPLAY_*`
- `ASTRA_MIGRATION_*`

`ScriptParity` 和 `RuntimeStress` 必须证明：

- save -> load -> replay hash 稳定。
- migration 缺失会阻止 release。
- mismatch report 至少定位到 frame、record kind 和 source object。



