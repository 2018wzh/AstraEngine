# Editor Workflow Blueprint

AstraEditor 的 v1 目标是 UE 级 creator workflow：同一项目从创建、导入、编辑、PIE、调试、打包到 Release Gate 都在一个工作流闭环中完成。Editor 不能拥有第二套 runtime model。

## Panel Contract

| Panel | Data Source | Main Actions | Required States |
| --- | --- | --- | --- |
| Project Wizard | template registry, project schema | create, open, validate | empty, loading, invalid schema, ready |
| Content Browser | AssetRegistry, import audit | import, reimport, reveal dependency, open sidecar | empty, stale cook, license blocked, ready |
| Script Editor | `.astra`, source map, diagnostics | edit, format, compile, jump to command id | parse error, conflict, ready |
| Graph Editor | visual metadata, policy metadata | edit node, bind command, save patch | no provider, conflict, ready |
| Timeline Editor | TimelineIr, Fence, media preview | edit clip, fence, preview, commit | missing asset, fence leak, ready |
| Inspector | PropertySystem metadata | edit property, reset, validate | read-only, invalid value, ready |
| PIE Viewport | RuntimeWorld public API | launch, pause, resume, step, stop | launching, paused, diagnostic break, running |
| Debugger | RuntimeDebugSession | inspect actor, event, state, source ref | no session, paused, running |
| Release Gate | release report schema | validate, explain, jump to source | running, blocked, pass |
| AI Review Queue | audit log, patch set | accept, reject, rollback | pending review, trusted applied, rejected |

## Bridge API

```rust
pub trait EditorRuntimeBridge {
    fn open_project(&mut self, path: ProjectPath) -> EditorResult<ProjectSessionId>;
    fn compile_story(&mut self, session: ProjectSessionId) -> EditorResult<CompileReport>;
    fn start_pie(&mut self, request: PieLaunchRequest) -> EditorResult<PieSessionId>;
    fn stop_pie(&mut self, session: PieSessionId) -> EditorResult<()>;
    fn validate_package(&mut self, request: PackageValidateRequest) -> EditorResult<ReleaseReportRef>;
}
```

Qt/QML 调 bridge；bridge 调 Runtime、Cook、Package、Release Gate。QML 不直接读写 package、save 或 Runtime internals。

## Source Round-trip

```text
.astra + project.yaml + policy metadata
  -> compile
  -> CompiledStory + source map
  -> visual model
  -> user edit
  -> source patch or policy override
  -> compile
  -> source map identity check
```

Identity check 失败时进入 Review Queue。Editor 必须显示 conflict，不生成新的 runtime truth。

## Checks

```bash
cargo test -p astra-editor-bridge project_wizard
cargo test -p astra-editor-bridge graph_timeline_edit
cargo test -p astra-editor-bridge release_gate_panel
```

Expected report: 每个 panel 能显示空、错误、运行、通过状态；失败项能跳转 source_ref 或 scenario action。
