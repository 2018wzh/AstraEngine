# UI Backend 实施规格

## Crate 与依赖方向

Migration 12 的目标 crate graph：

```text
astra-ui-core
  ├─ astra-vn-script
  ├─ astra-vn-ui
  ├─ astra-ui-yakui
  ├─ astra-vn-ui-yakui
  ├─ astra-ui-plugin-abi
  └─ platform renderer / Headless test host

astra-vn-core + astra-vn-system
  └─ astra-vn-ui
       └─ astra-vn-ui-yakui -> astra-ui-yakui -> yakui-core/widgets
```

`astra-ui-core` 只保存可序列化 input、semantic、action、theme、texture 和 mesh DTO。`astra-vn-ui` 保存 VN ViewModel builder、binding resolver、action router 和 Controller host。`astra-ui-yakui` 只做通用 Yakui adapter；`astra-vn-ui-yakui` 实现 Message、Choice、SaveSlot、Backlog、GalleryCard、RouteChart 和 AstraText 等 VN widget。

Yakui、Slint、taffy、wgpu、winit 类型不得从 adapter crate 的 public product contract 泄漏。AstraEMU 使用独立 `astra-emu-manager-ui-slint` adapter；Slint host 持有 winit event loop、surface 和同一套 wgpu 29.0.4 `Device`/`Queue`，family 只输出 renderer-neutral DTO。

## Script 编译

项目 source descriptor 为每个 `.astra` 文件声明 `Story` 或 `Ui` role。两个 role 共用 token、CST、formatter 和 source map；UI role lowering 为 `UiBlueprintBundle`：

```rust
pub struct CompiledVnProject {
    pub story: CompiledStory,
    pub ui_blueprints: UiBlueprintBundle,
    pub ui_bindings: VnUiBindingManifest,
    pub ui_controllers: VnUiControllerManifest,
    pub ui_themes: VnUiThemeManifest,
    pub source_maps: CompiledVnSourceMaps,
    pub hash: Hash256,
}
```

唯一产品编译入口是 `compile_astra_project`。UI semantic passes 按 `UiSymbols -> UiWidgets -> UiModels -> UiActions -> UiBindings -> UiCapabilities -> UiBundle` 执行。任一 pass 失败都不产出部分 project。

UI 表达式是受限 typed path，不执行脚本：`$model`、`$item`、`$event`、`$state`。复杂条件由 Rust ViewModel 或 Luau Controller 提前计算。静态可见文案必须使用 localization key；schema 标记的玩家输入、存档命名等动态值可以作为受控文本。

Luau Controller 使用生成的 `.d.luau` 和锁定的官方 `luau-analyze` 全量 typecheck。Controller effect 序列化后才能进入 host。允许 `none/session` state；load、locale/theme/profile generation 变化后按 manifest 重建。

## Package 与 target

`astra.target_manifest.v2` 对带 UI 的 Game/Program target 要求唯一 `ui_provider`。Package 直接使用以下 section：

```text
vn.compiled_project
vn.story_ir
vn.ui_blueprint_bundle
vn.ui_binding_manifest
vn.ui_source_map
vn.ui_controller_manifest
vn.ui_theme_manifest
ui.backend_manifest
ui.component_provider_manifest
ui.component_trust_manifest
ui.component_artifacts
```

reader 验证 root hash 与每个 child section hash、schema、provider binding 和 target/profile eligibility。旧 `vn.compiled_story` 与 target v1 直接拒绝并要求 recook，不做隐式升级。

## Runtime frame

```text
physical PlatformEvent / Headless input
  -> UiInputFrame
  -> binding resolver + Rust ViewModel
  -> Luau Controller effect
  -> Blueprint runtime
  -> Yakui layout/input/paint
  -> AstraText glyph plan
  -> UiSemanticSnapshot + UiRenderFrame
  -> SceneCommand/Mesh2D
  -> PlayerHostCommand::PresentScene
```

每个 input sequence 先进入 UI。`Consumed` 后停止；`Bubble` 才进入 VN input mapping。replay evidence 记录 input sequence、semantic target、action payload hash 和 disposition，不记录 Yakui WidgetId。

`PresentRgba` 仍可能服务其他平台 contract/测试，但 AstraVN UI 产品路径不得调用它。Migration 12 只扩展当前 `PresentScene` renderer-ready command stream，不建立第二个 composite presenter。

## Yakui adapter

adapter 从 Astra event 映射 viewport、pointer、wheel、keyboard、IME、touch 和 gamepad semantic navigation。若 upstream Yakui 缺少某个输入或 resource lifecycle 能力，preflight 必须阻断依赖选择；不得在 Astra contract 外维护无法复现的 patch。依赖通过后精确锁定并记录 build identity。

VirtualList/VirtualGrid 只实例化 visible range 与声明的 overscan。thumbnail 由 logical AssetRef 异步装载到 bounded LRU；pending/error 是显式 ViewModel state。context restore 清空 GPU generation 并全量重传 live texture，不吞掉失败。

## 产品页

Migration 12 必须使用同一 Blueprint/Controller/Backend 主路径完成：Message、Choice、Title、Config、Modal、Save/Load transaction、Backlog、Voice Replay、Gallery、Replay、Route Chart、Localization Preview 和 text-input fixture。Classic 与 Modern 是两个正式 profile，共享 Core state、action router 和 save/replay authority，只切换 binding/theme/presentation policy。

## 开发工具

`astra ui check` 校验 grammar、binding、action、controller、theme、localization、asset 和 capability。`preview` 使用 fixture ViewModel 打开真实 Yakui/Scene2D 页面；`snapshot` 输出 PNG、semantic、mesh 和 report；`matrix` 覆盖 locale、viewport、scale、input profile 和 text direction。

Dev hot refresh 只允许 Blueprint、Controller 和 Theme。编译失败立即终止 UI session并暂停 Core，由 host-owned diagnostic overlay 展示结构化错误。修复后创建新 UI session。发布 runtime 不热更新，plugin binary 仍禁止 hot reload。

## 可观测性

关键 span 至少包含 provider、target/profile、project hash、view id、session generation、input sequence、semantic hash、render hash、texture bytes、draw/vertex count 和 budget result。不得记录商业文本、玩家输入、clipboard、asset payload、绝对路径或整体 DTO。

## 验收矩阵

- Headless：相同 cooked package 和 physical input 形成 semantic/PNG/WAV/performance E2 preflight。
- Windows：pointer、keyboard、gamepad、IME/clipboard fixture、context restore、signed component dylib 和 hardware capture E3。
- Web：pointer、keyboard、touch、IME/clipboard fixture、WebGPU scene、signed component transpile artifact 和 browser capture E3。
- iOS/Android：本 migration 只关闭共享 touch contract/headless semantics；真实设备证据留 Stage 6。
- Accessibility：semantic tree、keyboard/gamepad、high contrast、font scale。AstraVN 不以平台 assistive-tech bridge 作为本 migration gate；AstraEMU Windows AccessKit 留 Stage 5。
