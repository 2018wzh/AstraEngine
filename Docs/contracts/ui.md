# UI Contract

本页是 Astra runtime UI 的公共契约。AstraVN、AstraEMU、PIE 和 Headless 可以共享本页类型，但产品 ViewModel、页面流程和第三方 backend 实现仍各自归属对应模块。

## 权威边界

- Core/runtime provider 持有剧情、save/load、config、backlog、unlock、route、replay 和 read-state 权威。
- Rust UI model builder 输出只读、schema-bound、已脱敏 ViewModel。
- `.astra` UI Blueprint 声明结构、binding、semantic id 和 action request。
- Luau Controller 只返回可序列化 UI effect，不直接修改 RuntimeWorld、save payload 或 provider session。
- UI backend 只持有 hover、focus、scroll、pointer capture、animation 和 texture cache 等瞬时状态。

UI action 必须经过 product action router 和 authority validation。`UnlockGallery`、直接 route cursor 写入、直接文件写入或原生 handle 操作不属于 UI action。

## 公共类型

Rust 类型是 schema 真源。实现至少提供：

```rust
pub struct UiBackendDescriptor {
    pub schema: String,
    pub provider_id: String,
    pub backend_name: String,
    pub backend_version: String,
    pub input_protocol: String,
    pub render_protocol: String,
    pub capabilities: Vec<String>,
    pub packaged_eligible: bool,
    pub headless_semantic_support: bool,
}

pub struct UiFrameInput {
    pub viewport: UiViewport,
    pub fixed_time_ns: u64,
    pub events: Vec<UiInputEvent>,
}

pub struct UiFrameOutput {
    pub actions: Vec<UiActionEnvelope>,
    pub input_dispositions: Vec<UiInputDisposition>,
    pub semantic_snapshot: UiSemanticSnapshot,
    pub render: UiRenderFrame,
    pub repaint_after_ns: Option<u64>,
}
```

`UiInputEvent` 只接受带 sequence 的 physical keyboard、IME、pointer、wheel、touch、gamepad、focus、resize、scale 和 fixed-time event。`UiInputDisposition` 对每个 sequence 明确 `Consumed` 或 `Bubble`。UI 已消费的输入不得再触发 VN advance/choose 或 legacy runtime command。

## Semantic tree

每个交互节点必须提供稳定 semantic id、role、bounds、state、允许的 action 和可选的脱敏 label key。ID 使用 page、slot、choice、command、gallery item、route node 等业务稳定键，不使用本地化文本、数组位置、地址或 backend WidgetId。

Semantic snapshot 用于 Headless、自动化、PIE 和 accessibility adapter。报告只保存 schema、stable id、role、state、bounds、action 和 hash；用户输入内容、商业文本、clipboard 内容和本地路径不得进入 report。

## Render frame 与资源生命周期

```rust
pub struct UiRenderFrame {
    pub schema: String,
    pub viewport: UiViewport,
    pub texture_updates: Vec<UiTextureUpdate>,
    pub texture_frees: Vec<UiTextureId>,
    pub primitives: Vec<UiMeshPrimitive>,
}
```

primitive 只包含 layer、clip、material、logical texture id、bounded vertex/index。颜色使用 premultiplied alpha；index、clip、texture region 和 byte size 在 host 侧再次校验。禁止 callback primitive、native texture handle 和 backend object。

UI frame 进入现有 renderer-ready `PresentScene`/Scene2D 主路径。Migration 12 可扩展 Mesh2D command，但不得让 AstraVN 回到 `PresentRgba` 或 CPU bitmap 产品 presenter。平台 context restore 后，provider 必须重发完整 live texture set；旧 generation texture id 立即失效。

## Text

正式文本由 Astra TextLayout 负责 font database、asset hash、fallback、grapheme、shaping、line break、CJK kinsoku、ruby、vertical glyph substitution、tate-chu-yoko、vertical ruby、clip 和 ellipsis。Yakui/egui 只分配容器并提供交互。

Migration 12 formal matrix 是 `zh-Hans`、`ja`、`en` 的横排，以及 `zh-Hans`/`ja` 的 CJK 竖排。BiDi/RTL 继续作为实现和 conformance 工作，未列入本 migration 的 release closure，不能据此标为 `DONE`。

## Theme 与组件

Theme 只使用 backend-neutral token：font、color、metric、texture、nine-slice、focus、motion 和 accessibility override。缺 token 或 asset 是 blocking diagnostic，不允许 backend 自行补默认皮肤。

内建 widget v1 至少包括 screen、row、column、stack、panel、image、nine-slice、text、rich-text、button、slider、toggle、select、scroll、virtual-list、virtual-grid、modal、canvas、semantic-region 和 text-input。作品专属动态组件遵循 [UI Component Plugin Contract](ui-component-plugin.md)。

## Provider binding

UI provider 由 target/profile/package 的显式 binding 唯一决定。descriptor、target、profile、capability、artifact fingerprint 和 package hash 必须一致。缺 binding、重复 binding、provider capability 不足或 fingerprint drift 均阻断，不得选择“第一个注册项”。

`astra.target_manifest.v2` 中有 UI 的 target 必须声明 `ui_provider`；无 UI 的 command-line/tool target 可以省略。Headless 使用 Migration 11 的 `HostLaunchProfile::Headless`，不能出现在 target platforms 或 cooked shipping profile。

## 性能与容量

formal release profile 的 blocking budget：

| 指标 | 上限 |
| --- | ---: |
| update + layout p95 | 2.0 ms |
| paint conversion p95 | 1.0 ms |
| stable frame texture update | 0 bytes/frame |
| draw calls | 128 |
| vertices | 250,000 |
| active UI textures | 64 MiB |
| Backlog data set | 10,000 entries |
| Backlog instantiated rows | 64 + declared overscan |

超限直接阻断。调整预算必须修改 release profile 并记录 ADR，不能降级成 warning。

## Release evidence

至少输出：`ui.backend.binding`、`ui.backend.capabilities`、`ui.input.consumption`、`ui.semantic_snapshot`、`ui.render_frame`、`ui.resource_restore`、`ui.text_layout`、`ui.theme`、`ui.performance`、`ui.accessibility` 和 `ui.visual_matrix`。Windows/Web 必须绑定同一 build/profile/package/provider/session；Headless 只形成 E2 preflight，不替代 E3。

