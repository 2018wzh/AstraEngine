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
    schema: astra.command.show.v1
  astra.vn.camera:
    provider: astra.vn.standard
    schema: astra.command.camera.v1
  astra.vn.movie:
    provider: astra.vn.standard
    schema: astra.command.movie.v1
```

## Command Matrix

| Command | Authoring Syntax | IR Output | Runtime Effect | Editor Metadata | Release Check |
| --- | --- | --- | --- | --- | --- |
| `show` | `show character:hero pose:normal at:center` | `PresentationCommand::SetSprite` | 创建或更新 sprite layer | actor picker、pose picker、anchor widget | asset ref、layer id、fallback |
| `hide` | `hide character:hero preset:fade` | `PresentationCommand::SetSprite` | 隐藏 sprite 或触发 exit timeline | target resolver、duration field | target exists、timeline closes |
| `move` | `move target:hero to:left duration:400` | `TimelineTask` | 插入 transform track | bezier/easing editor | join/cancel、skip policy |
| `camera` | `camera target:main zoom:1.1 duration:480` | `CameraState` + `TimelineTask` | 改变 stage camera | keyframe editor、safe area overlay | viewport bounds、budget |
| `transition` | `transition preset:crossfade duration:300` | `PresentationTimeline` | 场景或 layer transition | preset picker、preview | fallback、capture hash |
| `shake` | `shake target:camera.main strength:4 duration:180` | `CameraState` | deterministic shake | strength slider、curve picker | seed、skip snap |
| `movie` | `movie layer:video.opening asset:op01 end:wait` | `VideoLayerState` | 打开 video layer；`end:wait` 进入 `VnWaitState::Movie` | movie preview、end marker | decode capability、end token |
| `voice` | `voice asset:voice.hero.0001 sync:text` | `AudioCommand` + voice fence | 播放 voice 并绑定文本；sync 进入 `VnWaitState::Fence` | waveform preview | voice replay、auto wait |
| `bgm` | `bgm asset:bgm.room loop:true fade:600` | `AudioCommand` | 播放或切换 BGM bus | bus selector、loop marker | asset/license、loop point |
| `se` | `se asset:se.door bus:se` | `AudioCommand` | 播放短音效 | bus selector、gain slider | asset/license |
| `wait` | `wait ms:300` / `wait fence:voice_end` | `AwaitToken` | 进入 `VnWaitState::Fence` 或 timer await | fence picker | serializable token |
| `choice` | `choice key:prologue.where` | `RuntimeEvent::ChoiceOpen` | 进入 `VnWaitState::Choice`，等待 `choice.selected` payload | option list、route graph | reachability、savepoint |
| `system_page` | `system_page kind:save` | `SystemStoryCall` | 进入 `VnWaitState::SystemPage`，返回后恢复 cursor | system page picker | profile entry exists |

## Authoring Example

```astra
state prologue #@id state.prologue
  scene room #@id scene.room
    stage:
      show character:hero pose:normal at:center preset:hero_enter #@id hero.show
      camera target:main zoom:1.05 duration:480 #@id camera.push
      voice asset:voice.hero.0001 sync:text #@id voice.hero.0001
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
schema: astra.command.show.v1
fields:
  target:
    type: character_id
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
  duration_ms:
    type: u32
    default: 0
release_checks:
  - vn.commercial_baseline
  - presentation.asset_ref
  - presentation.fallback
```

Rust 类型是 schema 真源，YAML 只展示生成结果。schema 变更必须带 migrator 和 release gate evidence。

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
cargo test -p astra-vn standard_command_schema
cargo test -p astra-vn standard_command_ir
astra test run scenarios/full_playthrough.yaml --package target/nativevn.astrapkg --headless --report target/reports/vn-command.yaml
```

Expected report includes `vn.commercial_baseline`, `command.provider_binding`, `source_map.identity`, `timeline.join_cancel` and `system_stories.covered`.
