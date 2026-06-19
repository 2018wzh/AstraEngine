# Editor / Runtime / Creator Contract

状态：Production contract draft / Editor not implemented  
定位：定义 Editor PIE、Runtime Debugger、Inspector、Creator Workflow 与 Runtime public API 的连接边界。Editor 是工具层；packaged Runtime 不依赖 Editor。

## 1. 目标

- Editor 使用同一 RuntimeWorld、ScriptRuntimeHost、AssetRegistry、Media pipeline 和 Release Gate。
- PIE runtime changes 默认 transient；promote to source 必须生成 reviewable source patch proposal。
- Inspector、Debugger、Content Browser、Graph/Timeline 和 Cook/Package panel 使用 machine-readable diagnostics。
- Creator workflow 从 Template -> Project -> Content -> PIE -> Cook -> Package 闭合。

非目标：

- 不让 Editor 成为 runtime 发布前置条件。
- 不在 Runtime public API 中暴露 Editor widget、dock layout 或 UI object。

## 2. Editor Runtime Session

Session:

```yaml
schema: astra.editor.runtime_session.v1
session_id: editor-session:/nativevn/pie/1
mode: pie
runtime_profile: development
project_root: project:/
package_manifest_hash: ""
debug_enabled: true
preview_overlay_enabled: true
```

准接口：

```cpp
class IEditorRuntimeSession {
public:
    virtual Result<InspectResponse> Inspect(InspectRequest, DiagnosticSink&) = 0;
    virtual Result<DebugCommandResult> ExecuteDebugCommand(DebugCommand, DiagnosticSink&) = 0;
    virtual Result<void> ApplyPreviewOverlay(PreviewOverlay, DiagnosticSink&) = 0;
    virtual Result<SourcePatchProposal> PromotePreviewToSource(PromoteRequest, DiagnosticSink&) = 0;
};
```

## 3. Inspect And Debug

Inspect request:

```yaml
schema: astra.editor.inspect_request.v1
target:
  kind: actor
  id: actor:/characters/alice
include:
  - components
  - state_machine
  - scheduler_tasks
  - diagnostics
```

Debug command:

```yaml
schema: astra.editor.debug_command.v1
command_id: debug:/step-one-frame
kind: step_frame
target_runtime: editor-session:/nativevn/pie/1
breakpoints: []
```

Rules:

- Debug commands are recorded separately from packaged replay unless explicitly exported as test replay.
- Inspector response uses public DTOs and PropertySystem metadata.
- Runtime mutation commands require development profile or trusted Editor session.

## 4. Preview Overlay And Source Patch

Preview overlay:

```yaml
schema: astra.editor.preview_overlay.v1
overlay_id: overlay:/alice-position-preview
target: actor:/characters/alice
changes:
  - property: transform.x
    value: 128
save_policy: transient
```

Source patch proposal:

```yaml
schema: astra.editor.source_patch_proposal.v1
proposal_id: patch:/alice-position
source_file: Content/Scenes/Opening.scene.yaml
changes: []
review_required: true
diagnostics: []
```

Rules:

- Overlay never enters package.
- Promoted patch goes through Review Queue when property flags require review.
- Undo/redo applies to source edits and preview overlay separately.

## 5. Creator Workflow Contracts

Project Wizard output:

- project descriptor.
- selected EngineModuleSlot policy.
- template assets and sidecars.
- validation report.

Asset Import Wizard output:

- import preview.
- sidecar defaults.
- license/review state.
- source copy or mount policy.

Cook/Package panel output:

- cook report.
- package report.
- release gate report.
- blocking diagnostics.

`CreatorWorkflow` acceptance:

- create project from template.
- import image/audio/font/filter assets.
- run PIE against same RuntimeWorld path as package.
- package without Editor dependency.



