# AstraEmu Toolkit Contract

状态：Expansion toolkit contract draft / standalone runtime tool  
定位：定义 `AstraEmu Toolkit` 如何复用 AstraEngine 既有 runtime、module、asset package 和 media provider 管线。AstraEmu 是独立工具包，不进入 Astra Editor 或 NativeVN 制作流程。

## 1. 目标

- AstraEmu 只新增 Compat Core、probe result 和 TextCapture DTO；其他能力优先复用 AstraEngine 现有服务。
- AstraEmu 新增 package patch script/report contract，用于用户提供的原始发行数据结构描述和本地验收审计。
- ModuleRuntime 负责发现、加载、激活、停用和卸载 AstraEmu core/provider 模块。
- ServiceRegistry / ExtensionRegistry 暴露 Asset VFS、PackageReader、ScriptRuntimeHost、RuntimeWorld、Save/Replay、Renderer2D、TextLayout、Audio 和 FilterGraph 服务。
- EngineModuleSlot 选择 Compat Core、translation provider 和现有 media backends。
- 本地旧游戏目录和只读 package mount 默认 mount-only，不修改 foreign source。
- 首批 production family gate 覆盖 Artemis、KrKr/KAG/TJS/XP3 和 BGI/Ethornell。

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
| Original package adaptation | user-provided package patch script, sandbox, hash and diagnostics |
| Service access | ServiceRegistry capability views |
| Core registration | ExtensionRegistry `CompatRuntimeProvider` |
| Runtime stepping | RuntimeTickInput and Runtime scheduler ordering |
| Script-host integration | ScriptRuntimeHost-compatible provider path when useful |
| Visible output | RuntimeEvent and PresentationCommand |
| VN semantics | AstraVN Background / Character / Dialogue / Choice / Audio / Timeline categories |
| Rendering/text/audio/filter | Renderer2D, TextLayout, Audio and FilterGraph providers |
| Save/load | ISaveSectionProvider with opaque compat section payload |
| Core cold swap | ModuleRuntime deactivate -> shutdown -> unload -> load -> initialize -> activate |
| Local commercial acceptance | `astra.emu.local_case_report.v1` committed report without source payloads |

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
  - package_patch
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

Package patch result is a user-provided, sandboxed structural description for original-release data:

```yaml
schema: astra.emu.package_patch_result.v1
patch_id: local.krkr.senren_banka
engine_family: krkr
case_title: Senren Banka
hash: sha256:...
readers:
  - kind: xp3_index
    source: patch:/readers/xp3_index.lua
resource_maps:
  - foreign-krkr:/data/scenario.xp3#scenario/...
diagnostics: []
```

Mount rules:

- Local directories are mounted through Asset VFS as read-only `foreign-*:/` schemes.
- Existing Astra package data is consumed through PackageReader read-only package mounts.
- PackageReader can provide indexes, payload bytes and mount summaries; AstraEmu does not define another package reader contract.
- Probe may read filenames, headers and lightweight indexes.
- Probe must not mutate local content.
- Protected or encrypted content returns unsupported diagnostics.
- Package patch scripts may describe reader/index/decode/offset/table/resource-map logic, but must not bypass DRM or access control.

## 5. Compat Core Contract

`ICompatRuntimeProvider` is the only AstraEmu-specific runtime interface:

```cpp
class ICompatRuntimeProvider {
public:
    virtual CompatCoreDescriptor Describe() const = 0;
    virtual Result<CompatContentProbeResult> Probe(CompatProbeRequest, DiagnosticSink&) = 0;
    virtual Result<CompatPackagePatchResult> ApplyPackagePatch(CompatPackagePatchRequest, DiagnosticSink&) = 0;
    virtual Result<void> LoadContent(CompatContentMount, DiagnosticSink&) = 0;
    virtual Result<CompatStepResult> Step(RuntimeTickInput, DiagnosticSink&) = 0;
    virtual Result<void> WriteSaveSection(SaveWriteContext, DiagnosticSink&) = 0;
    virtual Result<void> ReadSaveSection(SaveReadContext, DiagnosticSink&) = 0;
    virtual Result<CompatCoverageReport> ExportCoverage(CompatCoverageRequest, DiagnosticSink&) = 0;
};
```

Execution rules:

- `Step` runs under Runtime tick/scheduler ordering and does not own the main loop.
- `Step` emits RuntimeEvent, PresentationCommand and optional TextCaptureEvent DTOs.
- RuntimeEvent sequence is assigned by Runtime, not by the core.
- Renderer, text, audio, decode and filter handles remain private to media providers.
- VM private state is opaque outside the compat save section.
- Core-specific control flow never becomes AstraVN native source syntax.
- Package patch scripts are input/config artifacts, not Compat Core private hard-coding.

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
- Save stores selected package patch hash and coverage checkpoint references when running a local commercial case.
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

## 8. Family v1 Contracts

首批 production acceptance 使用本地合法原始发行数据，不提交商业源文件：

| Family | Local case | Required package/data support | Required runtime coverage |
| --- | --- | --- | --- |
| Artemis | `Sakura no Uta 10th Anniversary Edition` | `.pfs`, `.pfs.000`, `.pfs.721`, movie data, fonts, `system.ini`, `.iet`, `.asb`, `.ast`, `.ipt`, `.sli`, `.tbl`, system Lua modules | `e:*` host API, tag executor, variables, call stack, waits, choices, image/text/audio/movie/system UI/save/load/backlog/replay |
| KrKr / KAG / TJS / XP3 | `Senren Banka` | `.xp3`, KAG scenario, TJS/system script, plugin metadata, media/font/save/config data | KAG tags, TJS required API, macros, choices, layers, media, movie, system menu, config, backlog, save/load/replay |
| BGI / Ethornell | `Subarashiki Hibi 15th Anniversary Edition` | BGI/Ethornell archives, scenario script, system script, media/font/save/config data | scenario/system script VM, route flow, choices, variables, media, movie, system menu, backlog, config, save/load/replay |

Full-content-flow coverage means 100% coverage for:

- main routes, branches, endings and choices.
- text, ruby/control metadata, voice, BGM and SE.
- CG, background, character sprite, transition, movie and visual effects.
- system menu, config, backlog, save/load and replay.

Uncovered required items block acceptance unless the local case report explicitly classifies the source data as protected/unsupported and the release profile accepts that failure as non-playable.

## 9. Local Case Report

```yaml
schema: astra.emu.local_case_report.v1
case_title: Senren Banka
engine_family: krkr
edition: original_release_local
local_root_hash: sha256:...
core_id: astra.emu.krkr.core
package_patch_set:
  id: local.krkr.senren_banka
  hash: sha256:...
probe_summary:
  confidence: 0.98
  entry_candidates: []
coverage:
  required_total: 0
  required_covered: 0
  uncovered_required: []
save_replay_summary:
  checkpoints: []
text_capture_summary:
  captured: 0
diagnostics: []
commands: []
```

Report rules:

- Report may be committed for audit.
- Commercial source files, unpacked payloads, private absolute paths and unauthorized screenshots must not be committed.
- Report records title/edition because these are the acceptance cases.
- Report records package patch hashes and provider feature hashes so local evidence is reproducible on a machine with the same legal data.

## 10. Core Cold Swap

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

## 11. Acceptance

- AstraEmu discovers Compat Core modules through ModuleRuntime and selects one through EngineModuleSlot.
- Local content is read through VFS or PackageReader read-only mounts.
- User-provided package patch scripts are sandboxed, hashed and audited.
- Artemis, KrKr and BGI local commercial cases each produce `astra.emu.local_case_report.v1`.
- Each required local case reaches 100% full-content-flow coverage, with zero uncovered required item.
- Compat state is saved and restored through ISaveSectionProvider.
- Core cold swap follows ModuleRuntime lifecycle and restores the previous module on failure.
- TextCaptureEvent reaches a selected translation provider and returns overlay PresentationCommand output.
- Existing Renderer2D, TextLayout, Audio and FilterGraph providers execute presentation and enhancement output.


