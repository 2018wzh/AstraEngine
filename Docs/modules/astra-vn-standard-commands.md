# AstraVN Standard Command Library

标准命令库把商业 VN 的常见表达收敛成稳定契约。`.astra` 写法面向创作者，IR 面向 Runtime，Luau policy 和插件负责具体表现。命令 provider 必须由项目 manifest 显式绑定，不能按加载顺序抢占。

## 命令契约

每个命令都必须声明 schema、authoring syntax、IR output、Runtime effect、Editor metadata 和 release check。

```rust
pub struct StandardCommandDescriptor {
    pub id: CommandId,
    pub provider: ProviderId,
    pub input_schema: SchemaRef,
    pub output_ir: IrKind,
    pub editor_metadata: EditorCommandMetadata,
    pub release_checks: Vec<CheckId>,
}
```

```yaml
commands:
  astra.vn.show:
    provider: astra.vn.standard
    schema: astra.command.show.v2
  astra.vn.camera:
    provider: astra.vn.standard
    schema: astra.command.camera.v2
  astra.vn.movie:
    provider: astra.vn.standard
    schema: astra.command.movie.v2
```

## Command Matrix

| Command | Authoring Syntax | IR Output | Runtime Effect | Editor Metadata | Release Check |
| --- | --- | --- | --- | --- | --- |
| `show` | `show id:hero asset:asset:/character/hero layer:characters at:center` | `StageCommand::Show` | 创建或更新 sprite layer | actor picker、pose picker、anchor widget | asset ref、layer id、fallback |
| `hide` | `hide id:hero preset:fade duration:200` | `StageCommand::Hide` | 隐藏 sprite 或触发 exit timeline | target resolver、duration field | target exists、timeline closes |
| `move` | `move id:hero x:320 y:0 duration:400` | `StageCommand::Move` | 写入确定性 transform intent | bezier/easing editor | target、duration、skip policy |
| `camera` | `camera target:main zoom:1.1 duration:480` | `StageCommand::Camera` | 改变 stage camera | keyframe editor、safe area overlay | viewport bounds、budget |
| `timeline` | `timeline id:tl.enter target:hero property:opacity keyframes:0=0,300=1 budget_ms:2` | `TimelineCommand::Start` | 创建有序 typed track；block join 必须带 fence | keyframe editor | keyframe 顺序、join/cancel、budget |
| `transition` | `transition preset:crossfade duration:300` | `StageCommand::Transition` | 通过显式 preset binding 运行场景 transition | preset picker、preview | provider binding、fallback、capture hash |
| `shake` | `shake target:camera.main strength:4 duration:180` | `StageCommand::Shake` | 生成 deterministic camera intent | strength slider、curve picker | seed、skip snap |
| `movie` | `movie layer:video asset:asset:/movie/op end:wait fence:movie.op.end fallback:asset:/movie/op_fallback` | `StageCommand::Movie` | 打开 video layer；`end:wait` 进入 `VnWaitState::MovieEnd` | movie preview、end marker | decode capability、fence、fallback |
| `voice` | `voice asset:asset:/voice/hero0001 sync:text` | `StageCommand::Audio` + voice fence | 播放 voice 并绑定文本 | waveform preview | voice replay、auto wait |
| `bgm` | `bgm asset:asset:/bgm/room loop:true fade:600` | `StageCommand::Audio` | 播放或切换 BGM bus | bus selector、loop marker | asset/license、loop point |
| `se` | `se asset:asset:/se/door bus:se` | `StageCommand::Audio` | 播放短音效 | bus selector、gain slider | asset/license |
| `wait` | `wait ms:300` / `wait fence:voice_end` | `AwaitToken` | 进入 `VnWaitState::Fence` 或 timer await | fence picker | serializable token |
| `choice` | `choice key:prologue.where` | `RuntimeEvent::ChoiceOpen` | 进入 `VnWaitState::Choice`，等待 `choice.selected` payload | option list、route graph | reachability、savepoint |
| `system_page` | `system_page kind:save` | `SystemStoryCall` | 进入 `VnWaitState::SystemPage`，返回后恢复 cursor | system page picker | profile entry exists |

## Authoring Example

```astra
state prologue #@id state.prologue
  scene room #@id scene.room
    stage viewport:1920x1080 safe_area:16:9 #@id stage.room
    layer id:characters kind:sprite z:100 blend:normal #@id layer.characters
    show id:hero asset:asset:/character/hero pose:normal layer:characters at:center preset:hero_enter #@id hero.show
    camera target:main zoom:1.05 duration:480 #@id camera.push
    voice asset:asset:/voice/hero0001 sync:text #@id voice.hero.0001
    text key:prologue.hello speaker:hero #@id line.hello
    wait fence:voice_end #@id wait.voice
    choice key:prologue.where #@id choice.where
      option key:choice.library -> library #@id choice.library
      option key:choice.rooftop -> rooftop #@id choice.rooftop
```

编译后输出稳定 command id、source span 和 IR hash；Editor 修改参数时只能回写 `.astra` 或 policy metadata。

`dialogue` 由 `text` command 产生：Core 写 backlog、read-state 和 voice replay，输出 TextWindow presentation，并进入 `VnWaitState::Dialogue` 等待玩家推进、auto 或 skip 规则。`choice.selected`、`player.advance` 和 `await.completed` 作为 Runtime event payload 进入 `astra.vn.step`，不通过全局 Blackboard 传递。

## Schema Example

```yaml
schema: astra.command.show.v2
fields:
  id:
    type: character_id
    required: true
  asset:
    type: asset_uri
    required: true
  pose:
    type: pose_id
    required: true
  layer:
    type: layer_id
    default: character
  preset:
    type: presentation_preset_id
    required: false
  duration:
    type: u32
    default: 0
release_checks:
  - vn.commercial_baseline
  - presentation.asset_ref
  - presentation.fallback
```

Rust 类型是 schema 真源，YAML 只展示生成结果。v1 的 raw attribute map 不再兼容；旧 package 在 decode 时返回 recook diagnostic。schema 变更必须同步 migration、release gate 和负向测试。

## Provider Binding

项目 manifest 必须显式选择命令 provider：

```yaml
command_providers:
  show: astra.vn.standard
  hide: astra.vn.standard
  move: astra.vn.standard
  camera: astra.vn.standard
  movie: astra.vn.standard
  system_page: astra.vn.system
```

两个插件声明同一命令时，未绑定项目直接阻断 package。绑定记录进入 package metadata 和 release report。

## Verification

```bash
cargo test -p astra-vn-script --test typed_stage_ir
cargo test -p astra-vn-commands --test standard_command_manifest
astra test run scenarios/full_playthrough.yaml --package target/nativevn.astrapkg --headless --report target/reports/vn-command.yaml
```

Expected report includes `vn.commercial_baseline`, `command.provider_binding`, `source_map.identity`, `timeline.join_cancel` and `system_stories.covered`.
