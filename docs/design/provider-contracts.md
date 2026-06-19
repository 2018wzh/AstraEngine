# Provider Contracts

状态：Production contract draft / not yet fully implemented  
定位：统一 ModuleRuntime、EngineModuleSlot、provider descriptor、capability negotiation、permission、hot reload、shutdown 和 ABI/release gate 的生产规则。本文补足 `extension-and-module-system.md` 的 provider 实现级细节。

## 1. 目标

- 所有可替换能力通过 provider descriptor 和 EngineModuleSlot 选择，不通过隐式链接或加载顺序抢占。
- Provider 必须声明 capability、required services、permissions、packaged eligibility、hot reload level 和 diagnostics prefix。
- Release Gate 可在不加载 unsafe binary 的情况下验证 descriptor、policy 和 provider dependency closure。
- Provider shutdown 可审计，不遗留 tasks、native resources 或 service registrations。

非目标：

- Provider 不能替换 Core diagnostics、ModuleManager ownership、PropertySystem 基础协议或 Runtime ownership。
- Provider public ABI 不传递 STL ownership、C++ object ownership、native handles 或 Editor widgets。

## 2. Descriptor

Provider descriptor:

```yaml
schema: astra.provider.descriptor.v1
provider_id: project.renderer.dx11
module_id: project.renderer
contract: IRenderer2DProvider
slot_id: astra.renderer2d
display_name: Project DX11 Renderer
version: 1
required_services:
  - astra.diagnostics
  - astra.asset.registry
capabilities:
  - texture_import
  - frame_capture
permissions:
  runtime:
    packaged: true
  platform:
    gpu: required
hot_reload:
  level: asset
release:
  packaged_eligible: true
  require_binary_hash: true
  require_abi_compatibility: true
diagnostics:
  code_prefix: ASTRA_RENDERER_DX11
```

Rules:

- `provider_id` unique per project.
- `slot_id` must exist before provider selection is valid.
- `contract` must match a known provider contract schema.
- `capabilities` are declarative and must be backed by validation or sample evidence before production release.

## 3. Capability Negotiation

Capability set:

```yaml
schema: astra.provider.capability_set.v1
provider_id: astra.decode.video.media_foundation
feature_hash: "..."
features:
  zero_copy:
    supported: true
    targets: [astra.renderer2d.d3d11]
  codecs:
    - h264.high.4_1
  headless_fallback:
    provider_id: astra.decode.video.headless_hash
```

Selection policy:

```yaml
schema: astra.provider.selection_policy.v1
slot_id: astra.video_decode
selected_provider: astra.decode.video.media_foundation
fallback_provider: astra.decode.video.cpu
required_features:
  - h264.high.4_1
optional_features:
  - zero_copy
```

Rules:

- Required features missing is blocking.
- Optional features missing becomes warning and fallback record.
- Feature hash enters cook key, package manifest and replay report.

## 4. Lifecycle And Shutdown

Lifecycle:

```text
Discovered -> DescriptorValidated -> BinaryValidated -> Loaded -> Initialized -> Active -> Deactivating -> Shutdown -> Unloaded
```

Shutdown contract:

```yaml
schema: astra.provider.shutdown_contract.v1
provider_id: project.renderer.dx11
owned_tasks: []
owned_resources: []
required_frame_boundary: true
timeout_ms: 2000
on_timeout: blocking_diagnostic
```

Rules:

- Provider must unregister services/extensions before unload.
- Runtime-owned persistent state must be handed to Save provider before shutdown.
- Hot reload replacement must prepare new provider before deactivating old provider when possible.

## 5. Permissions

Permission groups:

- `project_read`
- `project_write`
- `package_read`
- `package_write`
- `cache_read`
- `cache_write`
- `runtime_inspect`
- `runtime_mutate`
- `platform_gpu`
- `platform_audio`
- `network`
- `secret_read`

Rules:

- Runtime packaged profile denies `project_write`, authoring-only Editor permissions and debug MCP permissions.
- Provider cannot elevate permissions by resolving another service; ServiceRegistry audit must record requester module and permission check.
- AI/MCP providers require explicit review/audit permissions.

## 6. ABI And Release Gate

Release Gate checks:

- descriptor schema valid.
- binary hash present for packaged provider.
- ABI version compatible with engine.
- required services and dependencies available.
- selected provider belongs to requested slot.
- permissions allowed by release profile.
- hot reload level allowed by environment.
- no public native handle exposure.

Diagnostic prefixes:

- `ASTRA_PROVIDER_DESCRIPTOR_*`
- `ASTRA_PROVIDER_CAPABILITY_*`
- `ASTRA_PROVIDER_PERMISSION_*`
- `ASTRA_PROVIDER_SHUTDOWN_*`
- `ASTRA_PROVIDER_ABI_*`

`CustomizationPlugin` must include at least one provider selection, one invalid permission case and one ABI/release gate report.



