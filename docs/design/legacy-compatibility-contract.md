# Legacy Compatibility Contract

状态：Expansion contract draft / native runtime parity first  
定位：定义 Legacy compatibility expansion 的稳定边界：CompatRuntimeProvider、LegacyPackageReader、Legacy VM snapshot、SaveExtensionStateProvider 和 release gate。本文不展开具体旧引擎 opcode。

## 1. 目标

- Legacy runtime 使用稳定 Runtime、Asset、Media、Script、Save 和 FilterGraph API 接入。
- Legacy VM 可以维护私有 PC、stack、variables、call stack 和 timeline cursor，但必须通过 Save extension state 进入统一存档。
- Foreign assets 默认 mount-only，不反向污染 native AssetPipeline。
- Compatibility module 不要求修改 Core、Runtime、Asset、Media 的基础边界。

非目标：

- Legacy 不是 native runtime UE-class parity 的前置条件。
- 不把 Artemis/BGI/Kirikiri/Ren'Py VM control flow 并入 AstraVN native source language。

## 2. CompatRuntimeProvider

Descriptor:

```yaml
schema: astra.compat.runtime_provider.v1
provider_id: astra.compat.mock
contract: ICompatRuntimeProvider
slot_id: astra.compat.runtime
supported_engines: [mock]
packaged_eligible: false
diagnostics_prefix: ASTRA_COMPAT_MOCK
```

准接口：

```cpp
class ICompatRuntimeProvider {
public:
    virtual CompatRuntimeDescriptor Describe() const = 0;
    virtual Result<void> LoadProject(ForeignProjectRoot, DiagnosticSink&) = 0;
    virtual Result<CompatStepResult> Step(RuntimeTickInput, DiagnosticSink&) = 0;
    virtual Result<LegacyVmSnapshot> CaptureVmSnapshot(DiagnosticSink&) = 0;
    virtual Result<void> RestoreVmSnapshot(LegacyVmSnapshot, DiagnosticSink&) = 0;
};
```

Rules:

- Step emits RuntimeEvent and PresentationCommand; it does not call Renderer2D/Audio native handles.
- VM private state is opaque to AstraVN and saved as extension state.

## 3. Legacy Package Reader

Descriptor:

```yaml
schema: astra.compat.package_reader.v1
provider_id: astra.compat.artemis.package_reader
contract: ILegacyPackageReader
supported_extensions: [".pfs", ".iet", ".asb", ".ast"]
mount_policy: mount_only
```

准接口：

```cpp
class ILegacyPackageReader {
public:
    virtual Result<PackageMount> Mount(ForeignProjectRoot, MountPolicy, DiagnosticSink&) = 0;
    virtual Result<PackageIndex> Index(DiagnosticSink&) = 0;
    virtual Result<ByteBuffer> Read(ForeignAssetRef, DiagnosticSink&) = 0;
};
```

Rules:

- Illegal copy request is release-blocking.
- Foreign refs use explicit scheme, e.g. `foreign-artemis:/...`.
- Package index metadata can feed AssetResolver but does not become native sidecar unless imported by a reviewed modernization flow.

## 4. Save Extension State

Legacy VM snapshot:

```yaml
schema: astra.compat.legacy_vm_snapshot.v1
provider_id: astra.compat.mock
engine_id: mock
pc: ""
variables_hash: "..."
private_payload_schema: astra.compat.mock.vm_state.v1
private_payload: {}
```

Save extension provider:

```yaml
schema: astra.runtime.save_extension_provider.v1
provider_id: astra.compat.mock.save
section_id: section:/compat/mock
payload_schema: astra.compat.mock.vm_state.v1
migration_required: true
```

Rules:

- Legacy extension state cannot write native Actor/Component schema directly.
- Replay records emitted event hash and mapper version hash.

## 5. Release Gate And Acceptance

Release Gate checks:

- compat module disabled for native-only deterministic profile unless explicitly allowed.
- foreign mount policy respected.
- Save extension migration exists.
- mapper output uses Presentation/Runtime APIs only.

`CompatMockExpansion` acceptance:

- mock legacy project mounts read-only.
- VM emits RuntimeEvent and PresentationCommand.
- save/restore preserves opaque VM state.
- native runtime acceptance remains independent.

