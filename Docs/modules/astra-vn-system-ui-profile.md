# AstraVN System UI Profile

System UI 是商业 VN release 的阻断项。AstraVN Core 持有 save/load、config、backlog、read-state、gallery、replay、route chart 和 voice replay 的权威数据；Luau policy 只定义页面流程、样式、动画和可视化扩展。

当前实现仍使用 `SystemUiModel` 固定矩形与 hit-test。Migration 12 planned 主路径改为 `.astra` UI Blueprint + Rust ViewModel + typed Luau Controller + Yakui + AstraText + Scene2D；旧模型只有在 Windows/Web 正式证据闭合后才删除，不能把本页目标误写成已实现。UI 公共边界见 [UI Contract](../contracts/ui.md)。

## UI Blueprint binding

目标脚本入口使用显式 binding：

```astra
system_page kind:save view:ui.system.save controller:system.save policy:astra.policy.standard theme:astra.vn.theme.classic #@id page.save
```

Message、Choice 和 System Page 都可以有 command-specific binding；否则依次解析 system-page、surface `ui_bind` 与 profile。解析结果必须唯一。UI 只发送稳定 `option_id`、`slot_id`、`command_id`、`item_id` 或 `node_id` action，不用数组 index 或本地化文本作为身份。

## SystemStoryManifest

```rust
pub struct SystemStoryManifest {
    pub title: StoryRef,
    pub save: StoryRef,
    pub load: StoryRef,
    pub config: StoryRef,
    pub backlog: StoryRef,
    pub gallery: StoryRef,
    pub replay: StoryRef,
    pub route_chart: StoryRef,
    pub voice_replay: StoryRef,
    pub localization_preview: StoryRef,
}
```

```yaml
system_stories:
  title: system.title
  save: system.save
  load: system.load
  config: system.config
  backlog: system.backlog
  gallery: system.gallery
  replay: system.replay
  route_chart: system.route_chart
  voice_replay: system.voice_replay
  localization_preview: system.localization_preview
```

所有入口都必须能从 title、in-game menu 和 scenario action 打开。缺失入口阻断 `vn.system_ui_profile`。

## 数据模型

| 模块 | Core 数据 | Luau policy 可以做 | 不允许 |
| --- | --- | --- | --- |
| Save/Load | slot id、route cursor、VN save section、thumbnail ref、timestamp、playtime、version | 排序、筛选、缩略图布局、确认流程 | 改写 save section、隐藏 migration 错误 |
| Config | audio bus volume、text speed、auto delay、skip mode、language、accessibility | 面板布局、preset、preview | 写入未声明 system key |
| Backlog | command id、text key、speaker、voice ref、route position、read flag | 展示、搜索、jump request、voice replay request | 删除 Core backlog 记录 |
| Gallery | unlock source、asset ref、route flag、license tag | grid、filter、preview、unlock animation | 直接解锁未满足条件的内容 |
| Replay | scene replay id、required flags、media refs、policy snapshot ref | replay menu、preview、chapter grouping | 绕过 route/read-state 条件 |
| Route Chart | route node、edge、choice id、condition、ending | 图布局、hover 详情、jump request | 改写 route graph |
| Voice Replay | voice ref、speaker、text key、line id、asset availability | 播放 UI、角色筛选 | 播放未授权或缺失 asset |
| Localization Preview | locale、font fallback、ruby、line wrap、missing key | 并排预览、差异标记 | 把 preview 结果写成 runtime 文本 |

Classic 与 Modern 是两个正式 UI profile。它们共享 Core、save/replay 和 action authority，只切换 Blueprint binding、theme、presentation policy 和 UI session generation。关闭 Modern 后 Core hash 必须不变。

## Save Slot Metadata

```rust
pub struct SaveSlotMetadata {
    pub slot_id: SaveSlotId,
    pub story_id: StoryId,
    pub command_id: CommandId,
    pub route_label: RouteLabel,
    pub thumbnail: Option<AssetRef>,
    pub playtime_ms: u64,
    pub created_at_utc_ms: i64,
    pub schema_version: SchemaVersion,
    pub migration_status: MigrationStatus,
}
```

thumbnail 是 package 或 save section 中的 asset ref，不把截图 payload 写进报告。Release Gate 检查 save/load、migrator、thumbnail ref、slot bounds 和 replay hash。

## Config Schema

```yaml
schema: astra.vn.config.v1
audio:
  master: 0.8
  bgm: 0.7
  se: 0.8
  voice: 1.0
text:
  speed: 0.6
  auto_delay_ms: 1200
  skip_mode: read_only
display:
  language: zh-Hans
  font_scale: 1.0
  high_contrast: false
```

config 写入进入 `system` 变量域。schema 变更必须有 migrator；非法 key 或越界值阻断 package。

## Unlock Source

```rust
pub enum UnlockSource {
    RouteFlag { key: RouteFlagId },
    EndingReached { ending: EndingId },
    SceneRead { scene: SceneId },
    ExplicitCommand { command: CommandId },
}
```

gallery 和 replay 只读取 unlock source，不拥有单独权威状态。Luau 可以定义 reveal 动画和排序策略，不能自行解锁内容。

## Route Chart

Route chart 由 compiler 的 route graph 生成：

```yaml
schema: astra.route_chart.v1
nodes:
  - id: route.prologue
    kind: scene
    source_ref: main.astra#state.prologue
edges:
  - from: route.prologue
    to: route.library
    choice_id: choice.library
    condition: flag.library_open
```

Editor 可以保存 layout metadata；Runtime 只信 compiler graph 和 VN Core route state。

## Release Gate

`vn.system_ui_profile` 阻断条件：

- `SystemStoryManifest` 缺入口。
- system story 不可达，或不能返回原 story/savepoint。
- save/load/replay hash 不一致。
- config schema 缺 migrator 或含未声明 key。
- backlog、read-state、voice replay 与 Core state 不一致。
- gallery/replay unlock source 无法追溯到 route flag 或 command id。
- localization preview 缺 text key、font fallback 或 layout coverage。

```bash
astra test run scenarios/system_ui_profile.yaml --package target/nativevn.astrapkg --headless --report target/reports/system-ui.yaml
cargo test -p astra-vn system_ui_profile
```

Expected report includes `vn.system_ui_profile`, `system_stories.covered`, `save.load.replay`, `backlog.read_state`, `gallery.unlock_source` and `localization.preview`.
