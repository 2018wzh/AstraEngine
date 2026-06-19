# AstraEmu Toolkit Contract

状态：Expansion toolkit contract draft / standalone runtime tool  
定位：定义 `AstraEmu Toolkit` 如何复用 AstraEngine 既有 runtime、module、asset package 和 media provider 管线。AstraEmu 是独立工具包，不进入 Astra Editor 或 NativeVN 制作流程。

## 1. 目标

- AstraEmu 只新增 Compat Core、probe result 和 TextCapture DTO；其他能力优先复用 AstraEngine 现有服务。
- ModuleRuntime 负责发现、加载、激活、停用和卸载 AstraEmu core/provider 模块。
- ServiceRegistry / ExtensionRegistry 暴露 Asset VFS、PackageReader、ScriptRuntimeHost、RuntimeWorld、Save/Replay、Renderer2D、TextLayout、Audio 和 FilterGraph 服务。
- EngineModuleSlot 选择 Compat Core、translation provider 和现有 media backends。
- 本地旧游戏目录和只读 package mount 默认 mount-only，不修改 foreign source。

非目标：

- 不导入外部游戏为 Astra 项目。
- 不服务 Astra Editor 或 NativeVN 制作流程。
- 不描述创作者侧发布流水线。
- 不破解、解密或绕过 DRM / 商业保护。

## 2. Pipeline Reuse Map

| AstraEmu need | Reused AstraEngine contract |
| --- | --- |
| Core/module discovery | ModuleRuntime descriptor validation and lifecycle |
| Core selection | EngineModuleSlot provider selection |
| Local content access | Asset VFS read-only directory mount |
| Indexed packaged content | PackageReader read-only package mount |
| Service access | ServiceRegistry capability views |
| Core registration | ExtensionRegistry `CompatRuntimeProvider` |
| Runtime stepping | RuntimeTickInput and Runtime scheduler ordering |
| Script-host integration | ScriptRuntimeHost-compatible provider path when useful |
| Visible output | RuntimeEvent and PresentationCommand |
| VN semantics | AstraVN Background / Character / Dialogue / Choice / Audio / Timeline categories |
| Rendering/text/audio/filter | Renderer2D, TextLayout, Audio and FilterGraph providers |
| Save/load | ISaveSectionProvider with opaque compat section payload |
| Core cold swap | ModuleRuntime deactivate -> shutdown -> unload -> load -> initialize -> activate |

AstraEmu Manager is only a thin host facade over those services. It does not own a parallel runtime lifecycle, file system, save system, renderer, audio backend or package format.

## 3. Provider Descriptors And Slots

Compat Core descriptor follows provider descriptor shape:

```yaml
schema: astra.provider.descriptor.v1
provider_id: astra.emu.artemis.core
module_id: astra.emu.artemis
contract: ICompatRuntimeProvider
slot_id: astra.emu.compat_core
display_name: Artemis Compat Core
required_services:
  - astra.asset.vfs
  - astra.asset.package_reader
  - astra.runtime.world
  - astra.script.host
  - astra.save_replay
  - astra.presentation.library
capabilities:
  - content_probe
  - script_index
  - vm_debug
  - text_capture
  - save_section
  - cold_swap
permissions:
  project_write: false
  package_read: true
  runtime_inspect: true
hot_reload:
  level: cold_swap
diagnostics:
  code_prefix: ASTRA_EMU_ARTEMIS
```

Default slots:

- `astra.emu.compat_core`
- `astra.emu.translation_provider`
- reuse `astra.renderer2d`
- reuse `astra.text_layout`
- reuse `astra.audio`
- reuse `astra.filter_graph`
- reuse decode slots such as `astra.image_decode`, `astra.audio_decode`, `astra.video_decode`

Rules:

- Provider selection is explicit through EngineModuleSlot policy.
- Compat Core does not register renderer/text/audio replacements unless it is also a normal media provider.
- Translation provider is a plugin/provider, not part of the Compat Core ABI.

## 4. Content Probe And Mount

Probe is a Compat Core capability that reads through Astra VFS or PackageReader-style indexed access:

```yaml
schema: astra.emu.content_probe_result.v1
provider_id: astra.emu.artemis.core
root_uri: foreign-artemis:/
engine_family: artemis
engine_version: unknown
confidence: 0.92
entry_candidates:
  - system/first.iet
resource_roots:
  - image
  - sound
  - font
diagnostics: []
```

Mount rules:

- Local directories are mounted through Asset VFS as read-only `foreign-*:/` schemes.
- Existing Astra package data is consumed through PackageReader read-only package mounts.
- PackageReader can provide indexes, payload bytes and mount summaries; AstraEmu does not define another package reader contract.
- Probe may read filenames, headers and lightweight indexes.
- Probe must not mutate local content.
- Protected or encrypted content returns unsupported diagnostics.

## 5. Compat Core Contract

`ICompatRuntimeProvider` is the only AstraEmu-specific runtime interface:

```cpp
class ICompatRuntimeProvider {
public:
    virtual CompatCoreDescriptor Describe() const = 0;
    virtual Result<CompatContentProbeResult> Probe(CompatProbeRequest, DiagnosticSink&) = 0;
    virtual Result<void> LoadContent(CompatContentMount, DiagnosticSink&) = 0;
    virtual Result<CompatStepResult> Step(RuntimeTickInput, DiagnosticSink&) = 0;
    virtual Result<void> WriteSaveSection(SaveWriteContext, DiagnosticSink&) = 0;
    virtual Result<void> ReadSaveSection(SaveReadContext, DiagnosticSink&) = 0;
};
```

Execution rules:

- `Step` runs under Runtime tick/scheduler ordering and does not own the main loop.
- `Step` emits RuntimeEvent, PresentationCommand and optional TextCaptureEvent DTOs.
- RuntimeEvent sequence is assigned by Runtime, not by the core.
- Renderer, text, audio, decode and filter handles remain private to media providers.
- VM private state is opaque outside the compat save section.
- Core-specific control flow never becomes AstraVN native source syntax.

## 6. Save / Replay Reuse

Compat VM state is an `ISaveSectionProvider` section:

```yaml
schema: astra.runtime.save_section_descriptor.v1
section_id: section:/astra_emu/artemis
provider_id: astra.emu.artemis.core
payload_schema: astra.emu.artemis.vm_state.v1
required: false
```

Rules:

- Save stores opaque VM state, logical media state references, selected enhancement profile id and translation cache references.
- Save does not store native handles, threads, file descriptors, backend tokens or package reader internals.
- Replay records emitted event/presentation hashes and selected provider feature hashes through existing replay reports.
- Missing Compat Core leaves the section preserved but not executable.

## 7. Presentation, Enhancement And Translation

Presentation reuse:

- Images, text, audio, choices and effects map to AstraVN event categories and PresentationCommand.
- Text shaping uses TextLayout provider.
- Audio routing uses Audio provider logical buses.
- Layer-aware enhancement uses FilterGraph targets: background, character, ui, text and final.
- Decode, renderer, text, audio and filter fallback behavior follows existing provider capability reports.

TextCapture DTO:

```yaml
schema: astra.emu.text_capture_event.v1
provider_id: astra.emu.artemis.core
location: system/first.iet:120
speaker: Alice
text: 早上好。
text_hash: sha256:...
metadata:
  ruby: []
  control_tags: []
```

Translation rules:

- TextCaptureEvent is emitted beside RuntimeEvent / PresentationCommand.
- Translation provider is selected through `astra.emu.translation_provider`.
- Provider output becomes a PresentationCommand overlay by default.
- Embedded replacement requires explicit Compat Core capability.
- Translation cache is local toolkit state, not foreign source mutation.

## 8. Core Cold Swap

Cold swap uses existing ModuleRuntime lifecycle:

```text
Pause runtime
  -> Write compat save section
  -> Deactivate old module
  -> Shutdown old module
  -> Unload old module
  -> Load new module
  -> Initialize new module
  -> Activate new module
  -> Read compat save section
  -> Resume runtime
```

Rules:

- If the new module fails to load or activate, reload the old module and restore its save section.
- If the save section schema is incompatible, keep the runtime paused with diagnostics.
- If a media provider cannot rebuild resources, use normal provider fallback such as headless when available.
- Enhancement profile, translation config, font, filter and overlay data may reload through existing asset/config hot-reload paths without core swap.

## 9. Acceptance

- AstraEmu discovers Compat Core modules through ModuleRuntime and selects one through EngineModuleSlot.
- Local content is read through VFS or PackageReader read-only mounts.
- Mock Compat Core steps through RuntimeTickInput and emits RuntimeEvent / PresentationCommand.
- Compat state is saved and restored through ISaveSectionProvider.
- Core cold swap follows ModuleRuntime lifecycle and restores the previous module on failure.
- TextCaptureEvent reaches a selected translation provider and returns overlay PresentationCommand output.
- Existing Renderer2D, TextLayout, Audio and FilterGraph providers execute presentation and enhancement output.


