# AstraVN Presentation Model

本页定义 AstraVN 演出层的权威数据模型。参考 Ren'Py 的 script-first 可读性、KiriKiri/KAG 的 layer/tag 表达力、Naninovel/Unity 的服务化演出扩展，以及本仓旧引擎研究中的 stage、message layer、movie、transition 和 voice sync 经验；AstraVN 不复制任何外部 API，统一落到 `.astra`、CompiledStory IR、Luau policy 和 Runtime presentation command。

## 分层原则

商业 VN release 必须覆盖 commercial baseline 和 system UI profile。advanced presentation profile 是可选 gate，项目 opt-in 后才阻断发布。

| 层 | 运行时职责 | Editor 职责 | Release Gate |
| --- | --- | --- | --- |
| `StageModel` | 管理 viewport、safe area、layer stack、input priority、presentation budget | 显示舞台层级、z-order、锁定状态、预览尺寸 | `vn.commercial_baseline`、`vn.advanced_presentation` |
| `LayerState` | 保存 layer kind、blend、clip、mask、visibility、z-order | 支持拖拽排序、锁定、局部 preview | layer id 稳定，z-order 不冲突 |
| `CameraState` | 保存 position、zoom、rotation、shake、viewport、projection mode | Timeline scrub 和 keyframe 编辑 | camera task 必须 join/cancel |
| `SpriteState` | 保存 asset、pose、anchor、transform、opacity、filter chain | Inspector 修改 pose、anchor、preset | 缺资产、超预算 filter 阻断 profile |
| `TextWindowState` | 保存 layout、speaker、text effect、auto/skip 行为 | message window preview、font fallback | backlog/read-state/voice replay 一致 |
| `VideoLayerState` | 保存 movie ref、loop、alpha、end behavior、fallback frame | movie preview 和 end marker | decode capability 和 movie end await 可复现 |

## Rust Schema 真源

```rust
pub struct StageModel {
    pub stage_id: StageId,
    pub viewport: ViewportSpec,
    pub safe_area: SafeArea,
    pub layers: Vec<LayerState>,
    pub video_layers: Vec<VideoLayerState>,
    pub audio_commands: Vec<AudioCommand>,
    pub timeline_tasks: Vec<TimelineTaskState>,
    pub input_priority: Vec<InputLayerRef>,
    pub frame_budget: PresentationBudget,
}

pub struct LayerState {
    pub id: LayerId,
    pub kind: LayerKind,
    pub z_order: i16,
    pub visible: bool,
    pub blend: BlendMode,
    pub clip: Option<ClipRect>,
    pub mask: Option<AssetRef>,
    pub viewport: Option<ViewportSpec>,
}

pub struct CameraState {
    pub camera_id: CameraId,
    pub target_layer: LayerId,
    pub position: Vec2,
    pub zoom: f32,
    pub rotation_deg: f32,
    pub shake: Option<ShakeState>,
}

pub struct TextWindowState {
    pub window_id: TextWindowId,
    pub layout: TextLayoutRef,
    pub speaker: Option<CharacterId>,
    pub backlog_policy: BacklogPolicy,
    pub read_state_policy: ReadStatePolicy,
    pub voice_replay: Option<VoiceRef>,
}

pub struct VideoLayerState {
    pub layer_id: LayerId,
    pub movie: AssetRef,
    pub alpha: f32,
    pub loop_mode: LoopMode,
    pub end_behavior: MovieEndBehavior,
    pub fallback_frame: Option<AssetRef>,
    pub z_order: i16,
}

pub struct AudioCommand {
    pub command_id: CommandId,
    pub bus: AudioBus,
    pub asset: AssetRef,
    pub loop_mode: LoopMode,
    pub fade_ms: u32,
    pub sync: AudioSync,
}

pub struct TimelineTaskState {
    pub task_id: TimelineTaskId,
    pub timeline: PresentationTimeline,
    pub status: TimelineTaskStatus,
    pub cancel_reason: Option<String>,
}
```

这些类型通过 `serde` + `schemars` 生成 JSON Schema。YAML 和 `.astra` 只是作者源；Runtime 只消费 CompiledStory 中的 section。

当前 Stage 3 slice 已把 contract 收敛为 `SceneCommand`：纹理和 glyph 先显式 upload，sprite 只引用 resource id 与 source rect，资源必须显式 release；command stream 同时表达 glyph run、transform、camera、clip、opacity/blend、video frame 与 `FilterGraph`。`DrawCommand` 只保留兼容 type alias。`VnHeadlessPresentationExecutor` 作为 CPU reference 执行同一 stream并生成定位 hash；Windows hardware glyph subset 已由 platform host 执行并生成 GPU visual golden，但 sprite、camera、filter、video 等完整 Windows stream、WebGPU 与 formal Player evidence 尚未闭合，因此 `S3-PRESENT-01` 保持 `IN_PROGRESS`。

`ProductStageDirector` 是 typed `StageCommand` 的产品状态 owner。它用 fixed-point 保存 layer/entity/camera、tween、timeline、shake、movie/effect intent 和 frame identity；`apply`、`tick`、snapshot/restore 都采用事务提交，并受 package-bound profile budget 约束。NativeVN Player 已通过该 director 生成 background/sprite 的 retained `SceneCommand`，执行 safe-area clip、camera zoom/translation、opacity tween 和 texture lifecycle。当前平台 event loop 尚未把固定 frame tick送入 director，timeline completion 也尚未回到 Runtime await；movie/audio/effect、非 normal blend 和 camera rotation 仍会 blocking，不能由本地 director 单元测试外推为完整演出链。

## PresentationCommand

```rust
pub enum PresentationCommand {
    SetStage(StageModel),
    SetLayer(LayerState),
    SetCamera(CameraState),
    SetSprite(SpriteState),
    SetTextWindow(TextWindowState),
    SetVideo(VideoLayerState),
    PlayAudio(AudioCommand),
    RunTimeline(PresentationTimeline),
    CancelTimeline { task: TimelineTaskId, reason: CancelReason },
    CompleteTimeline { task: TimelineTaskId },
}

pub struct TimelineTask {
    pub task_id: TimelineTaskId,
    pub command_id: CommandId,
    pub tracks: Vec<TimelineTrack>,
    pub join_policy: JoinPolicy,
    pub skip_policy: SkipPolicy,
    pub auto_policy: AutoPolicy,
    pub replay_policy: ReplayPolicy,
    pub fallback: EffectFallback,
    pub budget: PresentationBudget,
}
```

`PresentationCommand` 只表达可序列化 effect。Luau policy 可以决定 task 编排、fallback 和 preset 选择，但不能直接操作 renderer、decoder、audio device 或平台 handle。

## Timeline / Director

Timeline 是 Director 的输入，不是第二套剧情状态。每条 track 必须绑定 command id、source span 和 rollback scope。

VN story 只在 `join_policy`、voice sync、movie end 或显式 `wait` 需要阻塞剧情时进入 `VnWaitState`。Director 可以继续执行或取消 timeline task，但不能直接移动 `VnCommandCursor`。

```yaml
schema: astra.presentation_timeline.v1
id: timeline.prologue.hero_enter
command_id: hero.enter
tracks:
  - id: camera.main
    kind: camera
    keyframes:
      - at_ms: 0
        position: [0, 0]
        zoom: 1.0
      - at_ms: 480
        position: [12, -8]
        zoom: 1.08
        tween: ease_out_cubic
  - id: sprite.hero
    kind: sprite
    keyframes:
      - at_ms: 0
        opacity: 0.0
      - at_ms: 360
        opacity: 1.0
join_policy: wait_all
skip_policy: snap_to_end
auto_policy: respect_voice_end
fallback:
  if_filter_missing: use_flat_fade
budget:
  max_ms_per_frame: 2.0
```

## Skip / Auto / Replay

| 模式 | 规则 |
| --- | --- |
| Skip unread | 只跳过 read-state 已读 command；未读文本停在 TextWindow commit 点 |
| Skip all | presentation task 执行 `skip_policy`，movie/audio 可以 fade out，但必须记录 deterministic event |
| Auto | 等待 text reveal、voice end、movie end 和作者配置的 auto delay；不读取墙钟 |
| Replay | 使用保存的 AwaitToken、Fence、seed 和 provider output；不重新请求 Luau 外部依赖或 AI provider |

voice sync 由 `TextWindowState.voice_replay`、AudioGraph voice channel 和 Timeline `voice_end` fence 共同完成。movie end 进入 `AwaitToken`，结果只在固定 tick 边界入队。

## Advanced Presentation Profile

项目 opt-in `vn.advanced_presentation` 后，Release Gate 额外检查 `vn.advanced_presentation_manifest` 和 scenario report：

- 多层 stage、camera、video layer、shader/filter 和 text effect 同时出现。
- 每个 timeline task 都有 join/cancel/fallback。
- voice sync、movie end、skip、auto 和 replay 都有 scenario 覆盖。
- Renderer2D provider 不支持某个 effect 时，fallback 不改变剧情状态。
- 性能预算以 headless capture、frame budget 和 provider capability report 为证据。

```bash
astra test run Examples/NativeVN/scenarios/route_rooftop.yaml --package target/nativevn.astrapkg --target nativevn-game --profile advanced-vn --headless --report target/reports/advanced-vn.yaml
```

Expected report includes `vn.advanced_presentation`, `timeline.join_cancel`, `presentation.fallback`, `voice.sync` and `renderer.effect_budget`.
