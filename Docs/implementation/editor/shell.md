# AstraEditor Shell 设计

AstraEditor shell 是编辑器的宿主层：负责窗口、Dock 布局、设计系统、Bridge 初始化、面板生命周期和快捷键。Shell 不持有任何 Runtime 内部状态；所有 Runtime 数据通过 `EditorRuntimeBridge` trait 的序列化 DTO 进入 QML。

## 1. 技术栈与构建

### 1.1 cxx-qt Bridge

使用 `cxx-qt`（KDAB，BSD-2-Clause）作为 Rust↔Qt/QML 的绑定层。Bridge 模块位于 `Editor/Source/Bridge/astra-editor-bridge/`。

核心 crate 依赖：

```toml
# Editor/Source/Bridge/astra-editor-bridge/Cargo.toml
[dependencies]
cxx-qt        = "0.7"
cxx-qt-lib    = "0.7"
astra-runtime           = { path = "../../../Engine/Source/Runtime/astra-runtime" }
astra-vn-editor         = { path = "../../../Engine/Source/Modules/AstraVN/astra-vn-editor" }
astra-vn-runtime-provider = { path = "../../../Engine/Source/Modules/AstraVN/astra-vn-runtime-provider" }
astra-plugin            = { path = "../../../Engine/Source/Runtime/astra-plugin" }
astra-release           = { path = "../../../Engine/Source/Developer/astra-release" }
astra-property          = { path = "../../../Engine/Source/Developer/astra-property" }
postcard                = "1"
serde_json              = "1"
tokio                   = { version = "1", features = ["rt-multi-thread", "sync", "time"] }

[build-dependencies]
cxx-qt-build = "0.7"
```

```rust
// build.rs
fn main() {
    cxx_qt_build::CxxQtBuilder::new()
        .file("src/lib.rs")
        .qrc("qml/qml.qrc")
        .build();
}
```

### 1.2 QML 数据绑定规则

| 数据类型 | Bridge 侧实现 | QML 消费方式 |
| --- | --- | --- |
| 列表/树（Content Browser、Plugin Manager、Inspector field list） | `QAbstractListModel` 子类（Rust impl） | `ListView`/`TreeView` model |
| 单对象状态（project 是否已开、active provider、PIE 状态） | `#[qproperty]` + `changed` signal | QML 属性绑定 |
| 复杂报告（Release Gate report、AI audit log、CompileReport） | JSON string property `#[qproperty(QString, report_json)]` | `JSON.parse(bridge.reportJson)` |

### 1.3 目录结构

```
Editor/
├── Source/
│   ├── App/
│   │   └── astra-editor/
│   │       ├── Cargo.toml
│   │       ├── build.rs
│   │       ├── src/
│   │       │   └── main.rs                 ← cxx-qt App 初始化入口
│   │       └── qml/
│   │           ├── qml.qrc
│   │           ├── AstraEditor.qml         ← 根窗口 + Dock 布局管理
│   │           ├── design/
│   │           │   ├── AstraDesignTokens.qml
│   │           │   └── AstraIcons.qml
│   │           ├── panels/
│   │           │   ├── ProjectWizard.qml
│   │           │   ├── ProjectSettings.qml
│   │           │   ├── ScriptEditor.qml
│   │           │   ├── GraphEditor.qml
│   │           │   ├── TimelineEditor.qml
│   │           │   ├── FilterGraphEditor.qml
│   │           │   ├── Inspector.qml
│   │           │   ├── PieViewport.qml
│   │           │   ├── Debugger.qml
│   │           │   ├── SaveReplayInspector.qml
│   │           │   ├── ContentBrowser.qml
│   │           │   ├── ReleaseGatePanel.qml
│   │           │   ├── PluginManager.qml
│   │           │   └── AiCopilot.qml
│   │           └── components/
│   │               ├── DockLayout.qml
│   │               ├── CommandPalette.qml
│   │               ├── TabBar.qml
│   │               ├── PluginQmlLoader.qml  ← 插件 QML 沙箱加载器
│   │               ├── ReviewQueue.qml
│   │               └── StatusBar.qml
│   ├── Bridge/
│   │   └── astra-editor-bridge/
│   │       ├── Cargo.toml
│   │       ├── build.rs
│   │       └── src/
│   │           ├── lib.rs                  ← cxx-qt bridge 声明 + EditorBridgeObject
│   │           ├── bridge.rs               ← EditorRuntimeBridge trait impl
│   │           ├── pie.rs                  ← PIE 线程与平台不透明表面管理
│   │           ├── inspector.rs            ← PropertySystem → QAbstractListModel
│   │           ├── debugger.rs             ← RuntimeDebugSession 封装
│   │           ├── plugin_manager.rs       ← ExtensionRegistrationReport → ListModel
│   │           ├── package_panel.rs        ← Release Gate 调用封装
│   │           ├── runtime_provider.rs     ← Provider 切换
│   │           ├── content_browser.rs      ← VFS catalog → ListModel + 缩略图
│   │           └── target.rs               ← Editor target 配置
│   └── Tests/
│       ├── project_wizard.rs
│       ├── pie_bridge.rs
│       ├── inspector_debugger.rs
│       ├── plugin_manager.rs
│       ├── graph_timeline_edit.rs
│       └── release_gate_panel.rs
```

---

## 2. 设计系统（AstraDesignTokens.qml）

UE5 风格深灰 + 蓝白 accent，中英双语 UI。

```qml
// Docs/implementation/editor/shell.md 参考配色
pragma Singleton
import QtQuick

QtObject {
    // ── 背景层级 ──────────────────────────────────────────────
    readonly property color bg0: "#111213"      // 最深：侧边栏/标题栏
    readonly property color bg1: "#1a1b1c"      // 主面板背景
    readonly property color bg2: "#222325"      // 工具栏/panel header
    readonly property color bg3: "#2c2d30"      // hover
    readonly property color bg4: "#383a3e"      // 选中/激活

    // ── Accent（UE5 蓝白）────────────────────────────────────
    readonly property color accent:       "#5b9bd5"
    readonly property color accentHover:  "#79b3e8"
    readonly property color accentActive: "#4a87c0"
    readonly property color accentText:   "#ffffff"

    // ── 文本 ─────────────────────────────────────────────────
    readonly property color textPrimary:   "#e8e8e8"
    readonly property color textSecondary: "#9e9e9e"
    readonly property color textDisabled:  "#555555"
    readonly property color textWarning:   "#e8b86d"
    readonly property color textError:     "#e05c5c"
    readonly property color textSuccess:   "#6dbf7e"

    // ── 边框 ─────────────────────────────────────────────────
    readonly property color border:      "#3a3c40"
    readonly property color borderFocus: "#5b9bd5"

    // ── 字体（捆绑 Noto Sans SC，三平台一致；等宽字体依赖系统）──
    // Editor 发行包捆绑 Noto Sans SC 子集（约 5 MB），确保 Linux 上中文可用。
    // Windows/macOS 将捆绑字体列在系统字体前，保证渲染一致性。
    readonly property string fontUI:   "Noto Sans SC, Inter, 微软雅黑, PingFang SC, WenQuanYi Micro Hei, sans-serif"
    readonly property string fontMono: "Fira Code, Cascadia Code, Consolas, Liberation Mono, monospace"
    readonly property int fontSizeSmall:  11
    readonly property int fontSizeNormal: 13
    readonly property int fontSizeLarge:  15

    // ── 间距 ─────────────────────────────────────────────────
    readonly property int spacingXS: 4
    readonly property int spacingS:  8
    readonly property int spacingM:  12
    readonly property int spacingL:  16
    readonly property int spacingXL: 24

    // ── 动画时长（ms）────────────────────────────────────────
    readonly property int animFast:   80
    readonly property int animNormal: 150
    readonly property int animSlow:   300
}
```

字体策略：Editor 发行包统一捆绑 Noto Sans SC 子集（~5 MB，仅常用 6000 汉字），列在字体 fallback 链首位，确保 Windows/macOS/Linux 三平台中文渲染一致。等宽字体（`fontMono`）依赖系统，Linux 额外列 Liberation Mono 作为 Fira Code 缺失时的 fallback。

> Noto Sans SC 子集由构建工具（`pyftsubset`）在 CI 中生成，存放于 `Editor/Resources/fonts/NotoSansSC-subset.ttf`，在 `AstraDesignTokens.qml` 加载前通过 `QFontDatabase::addApplicationFont()` 注册。

---

## 3. Dock 布局系统

### 3.1 预设布局（3 种）

| 布局名 | 主区域 | 左侧面板 | 右侧面板 | 底部面板 |
| --- | --- | --- | --- | --- |
| **Default** | Script Editor + Graph (Tab) | Content Browser | Inspector + Debugger (Tab) | PIE Viewport + Timeline (Tab) |
| **Scripting** | Script Editor（宽） | Content Browser | Inspector | — |
| **VN Graph** | Graph Editor（宽） | Content Browser | Inspector + Save/Replay Inspector (Tab) | Timeline + PIE Viewport (Tab) |

布局持久化：用户调整后，Dock 状态序列化为 JSON，存入 `{user_config_dir}/astra/editor/layout.json`。Qt 标准 `QDockWidget` 状态（`QMainWindow::saveState()`）用于实现，经 cxx-qt slot 在 Rust 侧读写。

Stage 4 不支持面板浮动为独立窗口（推迟到 Stage 6）。

### 3.2 Tab 合并规则

多个面板可以 Tab 合并在同一个 Dock slot（如 Graph + Timeline 共用底部区域）。Tab 标题显示面板名；激活 tab 的面板占据全部空间；`QTabWidget` 实现。

---

## 4. 面板生命周期与状态机

每个面板必须覆盖以下状态（参见 `editor-workflow.md` Panel Contract）：

```
empty → loading → ready ⇄ dirty ⇄ conflict
                       ↘ error
                       ↘ read-only
                       ↘ release-blocked
```

面板状态由 Bridge 侧的 DTO 驱动，QML 侧通过 `switch` 语句或 `StackLayout` 渲染不同状态视图。Bridge 不直接持有 QML 对象引用，只发 signal。

---

## 5. PIE Viewport

### 5.1 AstraPlatform 收束与平台隔离设计

PIE 使用同进程独立线程（Stage 4）；Stage 6 可升级为独立子进程。

按照 [Platform Host Blueprint](../platform-host.md) 约束，AstraEngine 的公共模块与 Editor 均不得泄露低层原生窗口句柄（如 `HWND`、`NSView*`、`XID` 等）。任何平台特定的窗口嵌入与 wgpu Surface 创建细节，必须统一收束在 `astra-platform` 及各平台具体实现中。

Editor 侧通过 `AstraPlatform` 提供的通用 API 关联窗口与渲染器：

```rust
// Editor/Source/Bridge/astra-editor-bridge/src/pie.rs

use astra_platform::{PlatformHost, SurfaceRequest, SurfaceToken};

pub struct PieThreadHandle {
    thread:     JoinHandle<()>,
    control_tx: mpsc::Sender<PieControl>,
    // 权威的平台隔离 Surface 凭证，核心 Editor 不持有任何原生句柄
    surface_token: SurfaceToken,
}

pub fn spawn_pie(
    request: PieLaunchRequest,
    win_id:  u64,                // QWidget::winId() 暴露出的平台不透明句柄
) -> EditorResult<PieThreadHandle> {
    // 1. 获取当前平台对应的 PlatformHost 实例
    let mut host = astra_platform::get_active_host();

    // 2. 构造平台通用的 Surface 关联请求
    let surface_req = SurfaceRequest {
        window_handle: win_id,
        size_width:    request.width,
        size_height:   request.height,
        // 是否允许离屏 fallback (Wayland 等无嵌入能力环境下的自动判定由 astra-platform 隐式处理)
        allow_fallback: true,
    };

    // 3. 由 astra-platform 原生适配层创建 Surface 并返回不透明 Token
    // 原生句柄到 wgpu Surface 的内部映射在 astra-platform-windows / macos / linux 内闭环完成
    let surface_token = host.create_surface(surface_req)?;

    // 4. 启动 tokio Runtime 并将 surface_token 传入 RuntimeWorld 进行渲染绑定
    // 5. 启动 PIE 线程，主循环执行 tick
    ...
}
```

### 5.2 各平台底层实现收束（宿主层隐藏）

原生窗口句柄的具体解析和降级策略完全封装在 `astra-platform-*` 内，不对 Editor 泄露：

| 平台 | 适配器 Crate | astra-platform 内部处理逻辑 | Fallback 降级表现 |
| --- | --- | --- | --- |
| **Windows** | `astra-platform-windows` | 将 `win_id` 转为 `HWND` 并创建 Win32 wgpu Surface | 无需降级 |
| **macOS** | `astra-platform-macos` | 将 `win_id` 转为 `NSView*` 并创建 AppKit wgpu Surface | 无需降级 |
| **Linux X11** | `astra-platform-linux` | 检测到 X11 环境时，将 `win_id` 作为 `XID` 创建 XCB Surface | 无需降级 |
| **Linux Wayland**| `astra-platform-linux` | 检测到 Wayland 原生环境（Wayland 协议限制 child window embed）时，在 `create_surface` 内部自动透明降级：转为离屏 FBO 渲染，并以 `SurfaceToken::TextureShared` 抛出 | **自动开启 Texture 共享路径**：渲染结果写回共享的 GLES2/Vulkan texture，供 Qt 在 C++ 宿主侧使用 `QSGSimpleTextureNode` 渲染呈现 |

### 5.3 QML 与 宿主 C++ 视图桥接

QML 视图层只使用由 `AstraPlatform` 报告的 `PlatformCapabilityReport` 决定呈现策略：

```qml
// panels/PieViewport.qml
Item {
    id: pieViewportRoot

    // NativeRenderArea：Qt Quick C++ 桥接组件
    NativeRenderArea {
        id: renderArea
        anchors.fill: parent

        onWindowIdReady: function(winId, w, h) {
            // 直接将不透明 winId 传给 Bridge，不判断平台细节
            bridge.startPie(winId, gameTarget, providerId, profile)
        }
        
        onResized: function(w, h) {
            bridge.resizePie(w, h)
        }
    }

    // Wayland texture 接收层（只有当 astra-platform 判定启用并降级为 texture 共享时激活）
    WaylandTextureLayer {
        visible: bridge.pieIsTextureSharedMode
        textureHandle: bridge.waylandPieTextureHandle
        anchors.fill: parent
    }

    // 控制条
    PieControlBar {
        anchors.bottom: parent.bottom
        state: bridge.pieState   // "idle" | "launching" | "running" | "paused" | "diagnostic_break"
        onPlay:   bridge.resumePie()
        onPause:  bridge.pausePie()
        onStop:   bridge.stopPie()
        onStep:   bridge.stepPie()
    }

    // 诊断断点 overlay
    PieDebugOverlay {
        visible: bridge.pieState === "diagnostic_break"
        diagnosticJson: bridge.currentDiagnosticJson
    }
}
```

### 5.4 PIE Session Lifecycle

```
create(game_target, provider, profile)
  → validate target + provider binding 
  → 调用 astra_platform::get_active_host().create_surface() 关联平台视口
  → 根据 SurfaceToken 类型确定宿主渲染路径 (NativeEmbed 或 TextureShared)
  → signal: pie_state_changed("launching")
  → RuntimeWorld::run() → signal: pie_state_changed("running")
  → [pause] → signal: pie_state_changed("paused")
  → [diagnostic break] → signal: pie_state_changed("diagnostic_break")
  → [stop] → thread join → signal: pie_state_changed("idle")
```

PIE 使用 `Game` target（不是 `Editor` target）。Editor target（`kind = editor, packaged = false`）只用于 Editor 自身的 RuntimeWorld 初始化，不出现在 game package 中。

```
create(game_target, provider, profile)
  → validate target + provider binding → detect platform → start thread → wgpu init
  → signal: pie_state_changed("launching")
  → RuntimeWorld::run() → signal: pie_state_changed("running")
  → [pause] → signal: pie_state_changed("paused")
  → [diagnostic break] → signal: pie_state_changed("diagnostic_break")
  → [stop] → thread join → signal: pie_state_changed("idle")
```

PIE 使用 `Game` target（不是 `Editor` target）。Editor target（`kind = editor, packaged = false`）只用于 Editor 自身的 RuntimeWorld 初始化，不出现在 game package 中。

---

## 6. Inspector

### 6.1 PropertySystem → QAbstractListModel

`astra-property` 的 `PropertySystem` metadata 在 Bridge 侧映射为一个 `QAbstractListModel`，每行对应一个 property field：

```rust
pub struct InspectorFieldRow {
    pub display_name: String,         // 面板显示名（支持中英）
    pub property_kind: PropertyKind,  // 决定 QML 渲染哪种控件
    pub current_value_json: String,   // JSON 序列化当前值
    pub is_read_only: bool,
    pub validation_error: Option<String>,
}
```

QML 的 `Inspector.qml` 用 `DelegateChooser` 根据 `property_kind` 选择渲染控件：

| `PropertyKind` | QML 控件 |
| --- | --- |
| `string` | `TextField` |
| `number` | `SpinBox` 或 `Slider`（由 meta hint 决定） |
| `boolean` | `CheckBox` |
| `enum` | `ComboBox`（选项从 meta hint 读取） |
| `color` | 自定义 `ColorPicker`（QML） |
| `asset_ref` | `AssetPicker`（调用 `AstraEditor.FilePicker`） |
| `vec2`/`vec3` | 多字段 inline `SpinBox` 组合 |
| `array` | `ListView` + add/remove 按钮 |
| `map` | key-value 双列表 |
| 插件扩展类型 | `PluginQmlLoader`（沙箱加载插件提供的 Inspector widget QML） |

Stage 4 不支持多选批量编辑；Inspector 每次只显示单个对象的属性。Stage 5 后实现 `PropertySystemBatchEdit`。

---

## 7. Debugger

`astra-runtime` 的 `RuntimeDebugSession` API 已在 Stage 1 实现。Debugger Bridge（`debugger.rs`）：

1. 通过 `RuntimeDebugSession` 查询 Actor 列表、Component 状态、StateMachine 当前 state/transition、EventQueue、AwaitToken、Blackboard。
2. 把 trace span 映射回 `.astra` source map（调用 `astra-vn-editor` 的 `SourceMapRef`）。
3. 以 JSON DTO 发 signal → QML `Debugger.qml` 渲染。

```qml
// panels/Debugger.qml
SplitView {
    // 左：Actor 树
    ActorTree { model: bridge.debugActorModel }

    // 右：选中 Actor 的 Component/StateMachine 详情
    DebugDetail {
        actorJson: bridge.selectedActorJson
        onSourceRefClicked: function(sourceRef) {
            // 跳转 Script Editor 到对应行
            scriptEditor.revealSourceRef(sourceRef)
        }
    }
}
```

---

## 8. Content Browser

### 8.1 VFS 树 → QAbstractListModel

VFS catalog（`astra-asset`）在 Bridge 侧映射为树型 `QAbstractItemModel`（懒加载）：

```rust
// content_browser.rs
pub struct VfsTreeModel { ... }  // impl QAbstractItemModel

impl VfsTreeModel {
    // 懒加载：fetchMore() 时从 VFS provider 读取子节点
    // 节点类型：Directory | Asset(kind, cook_state, thumbnail_key)
}
```

### 8.2 缩略图缓存

- 缓存目录：`{project_dir}/.astra-cache/thumbnails/{asset_id_hash}.png`
- 缓存 key：`{asset_id}:{cook_version}` —— cook 版本变化时自动失效，重新生成
- 生成：后台 Tokio 线程；生成完成后发 `extension_list_changed` 或专用 `thumbnailReady(assetId)` signal，QML 侧刷新对应 delegate
- 脏资产（未 cook / 导入中）：每次 Editor 会话重生成，不写持久化缓存

### 8.3 拖拽导入

OS 文件拖拽 → QML `DropArea` → Bridge `import_asset(Vec<PathBuf>, importer_hint)` slot → `astra-cook` Importer → cook + 缩略图生成 → VFS catalog 刷新 → `extension_list_changed`

---

## 9. Plugin Manager

### 9.1 数据来源

Bridge 的 `plugin_manager.rs` 只读取 Stage 1/2 已产出的 report：
- `ExtensionRegistrationReport`（来自 `astra-plugin` PluginRegistrar）
- `plugin.extension_registry`（JSON report）
- `plugin.dependency_graph`（JSON report）

不实现第二套 dependency graph 或 provider selection 逻辑。

### 9.2 QML 插件 QML 沙箱（PluginQmlLoader）

插件可以提供 `.qml` 文件作为自定义面板或 Inspector widget。加载规则：

```qml
// components/PluginQmlLoader.qml
Loader {
    id: loader
    // 沙箱：独立的 QQmlContext，只暴露白名单 context property
    property QtObject sandboxContext: QtObject {
        // 允许：只读 Editor 主题 token
        property var tokens:     AstraDesignTokens
        // 允许：只写 panel 注册（声明式，无 JS 回调）
        property var panelApi:   PluginPanelApi {}
        // 允许：调用 Editor 文件选择器（走 Bridge，不直接访问文件系统）
        property var filePicker: AstraEditorFilePicker {}
    }

    // import 白名单（禁止 QtQuick.Dialogs、Qt.labs.process、
    //               QtQuick.LocalStorage、Qt.WebSockets、QtWebEngine）
    // 通过 QQmlEngine::setImportPathList() 限制搜索路径实现
    onStatusChanged: {
        if (status === Loader.Error) {
            pluginError = errorString()
        }
    }
}
```

插件 QML 可提供的扩展类型：

| 扩展类型 | QML 合约 | 安全级别 |
| --- | --- | --- |
| 独立面板（Panel） | `AstraPanel { panelId; title; content: Item }` | 低风险 |
| Graph Node 外观 | `AstraGraphNodeStyle { nodeType; icon; color; portSchema }` | 低风险 |
| Timeline Track 外观 | `AstraTimelineTrackStyle { trackKind }` | 低风险 |
| Inspector Widget | `AstraInspectorWidget { propertyType; editor: Item }` | 中风险 |
| PIE Viewport Overlay | `AstraPieOverlay { content: Item }` | 高风险（需 `pie_overlay` 权限声明） |
| Theme Override | `AstraThemeOverride { tokenOverrides: var }` | 低风险 |

---

## 10. Release Gate 面板

Package/Release Gate panel（`ReleaseGatePanel.qml`）调用 `astra-release` crate 中的同一 CLI validator。Bridge 侧 `package_panel.rs` 将 `ReleaseReport` 序列化为 JSON DTO，QML 侧按 check 列表渲染。

状态机：`idle → running → (blocked | warning | pass)`

blocked check 显示：check id、diagnostic 详情、source span（可点击跳转 Script Editor 或 scenario action）、可复现 CLI 命令。

---

## 11. Runtime Provider 切换

Bridge 侧 `runtime_provider.rs` 实现：

```rust
// S4-EDITOR-RUNTIME-PROVIDER-01
fn list_runtime_providers(&mut self, session: ProjectSessionId)
    -> EditorResult<Vec<ProductRuntimeDescriptor>>;

fn read_runtime_editor_metadata(
    &mut self, session: ProjectSessionId,
    provider: ProviderId, profile: ProfileId,
) -> EditorResult<RuntimeEditorMetadata>;

fn set_active_editor_target(
    &mut self, request: EditorTargetSelectionRequest,
) -> EditorResult<RuntimeEditorMetadata>;
```

`RuntimeEditorMetadata.authoring_surfaces` 决定 QML 侧显示哪些面板：

```
NativeVnRuntimeProvider → .astra Script Editor, VN Graph, Timeline, System UI, Luau policy
AstraEmuRuntimeProvider → (planned) legacy trace, text/translation overlay, FilterGraph preset
AstraRpgRuntimeProvider → (planned) Map, Quest, Battle, Behavior Graph
```

Editor shell 根据 `authoring_surfaces` 列表动态显示/隐藏面板；未绑定 runtime 的专属面板显示「该玩法包未启用」空状态提示。

---

## 12. i18n（中英双语）

Stage 4 起使用 Qt Linguist（`.ts`/`.qm` 文件）支持中英双语 UI。

```
Editor/Source/App/astra-editor/i18n/
├── astra-editor_zh_CN.ts   ← 中文翻译源
└── astra-editor_en.ts      ← 英文（基准字符串）
```

所有 QML 字符串使用 `qsTr()`/`qsTranslate()` 包裹；Rust 侧 display name 在 Bridge DTO 中以 `LocalizedString { zh: String, en: String }` 格式传递，QML 根据当前语言设置选择字段。

---

## 13. 快捷键（Stage 4 固定表）

| 操作 | 快捷键 |
| --- | --- |
| 命令面板 | `Ctrl+P` |
| 查找（Script Editor） | `Ctrl+F` |
| 替换（Script Editor） | `Ctrl+H` |
| 编译故事 | `Ctrl+Shift+B` |
| 启动 PIE | `Alt+P` |
| 暂停/恢复 PIE | `Alt+Space` |
| 停止 PIE | `Alt+Shift+P` |
| Undo | `Ctrl+Z` |
| Redo | `Ctrl+Y` / `Ctrl+Shift+Z` |
| 保存 | `Ctrl+S` |
| 全局 Undo | `Ctrl+Shift+Z`（全局 patch 历史） |
| 切换布局 Default | `Ctrl+F1` |
| 切换布局 Scripting | `Ctrl+F2` |
| 切换布局 VN Graph | `Ctrl+F3` |

Stage 5 实现可定制快捷键（Settings 面板 + JSON 持久化）。

---

## 14. Undo/Redo 分层栈

```
Script Editor（ropey 局部 undo）
    ↓ commit（compile 触发时）
全局 patch 历史（按 patch id + source map 执行）
    ← Graph Editor 操作（直接进全局栈）
    ← Timeline Editor 操作（直接进全局栈）
    ← Inspector 操作（直接进全局栈）
    ← AI 写入（AI Review Queue Apply 后的 undo checkpoint 进全局栈）
```

全局栈最大深度：100 条 patch（可在 Project Settings 中配置，Stage 5）。

---

## 15. 构建集成

```
# 正常开发（不需要 Qt）
cargo test --workspace

# Editor 开发（需要 Qt 6.5 LTS 安装）
# 设置 Qt6_DIR 或 QTDIR 后：
cargo build -p astra-editor-bridge
cargo build -p astra-editor
cargo test  -p astra-editor-bridge project_wizard
cargo test  -p astra-editor-bridge editor_creator_loop
```

CI 配置（GitHub Actions 三平台 matrix 样例）：

```yaml
# .github/workflows/editor.yml
jobs:
  editor:
    strategy:
      matrix:
        include:
          - os: windows-latest
            qt_arch: win64_msvc2019_64
            qt_modules: "qtmultimedia"
            rust_target: x86_64-pc-windows-msvc
          - os: macos-14          # Apple Silicon runner
            qt_arch: clang_64
            qt_modules: "qtmultimedia"
            rust_target: aarch64-apple-darwin
          - os: ubuntu-22.04
            qt_arch: gcc_64
            qt_modules: "qtmultimedia"
            rust_target: x86_64-unknown-linux-gnu
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4

      - name: Install system deps (Linux)
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libxcb-xkb-dev libxkbcommon-x11-dev \
            libxcb-icccm4-dev libxcb-image0-dev libxcb-keysyms1-dev \
            libxcb-randr0-dev libxcb-render-util0-dev libxcb-shape0-dev \
            libxcb-sync-dev libxcb-xfixes0-dev libxcb-xinerama0-dev \
            libgl1-mesa-dev libegl1-mesa-dev libvulkan-dev

      - name: Install Qt 6.5 LTS
        uses: jurplel/install-qt-action@v3
        with:
          version: '6.5.*'
          arch: ${{ matrix.qt_arch }}
          modules: ${{ matrix.qt_modules }}

      - name: Generate Noto Sans SC font subset (Linux)
        if: runner.os == 'Linux'
        run: |
          pip install fonttools brotli
          pyftsubset Editor/Resources/fonts/NotoSansSC-Regular.ttf \
            --output-file=Editor/Resources/fonts/NotoSansSC-subset.ttf \
            --unicodes-file=Editor/Resources/fonts/cjk_subset.txt

      - name: Build Editor
        env:
          Qt6_DIR: ${{ env.Qt6_DIR }}
        run: cargo build -p astra-editor-bridge -p astra-editor --target ${{ matrix.rust_target }}

      - name: Test Editor Bridge
        env:
          QT_QPA_PLATFORM: offscreen   # CI 无显示器，使用 offscreen 后端
        run: |
          cargo test -p astra-editor-bridge project_wizard
          cargo test -p astra-editor-bridge editor_creator_loop
          cargo test -p astra-editor-bridge plugin_manager
```

---

## 16. 跨桌面平台注意事项

本节记录 Windows / macOS / Linux 三平台差异和对应处理策略。

### 16.1 平台支持矩阵（Editor target）

| 平台 | 版本基线 | 状态 | Qt QPA backend | 备注 |
| --- | --- | --- | --- | --- |
| **Windows** | Windows 10 22H2+ | Stage 4 一级目标 | `windows` (Win32) | WMF decode、WASAPI、DirectX 12 wgpu backend |
| **macOS** | macOS 13 Ventura+ | Stage 4 同步支持 | `cocoa` (AppKit) | Metal wgpu backend；Xcode 15+ toolchain |
| **Linux X11** | Ubuntu 22.04 LTS / Fedora 38+ | Stage 4 同步支持 | `xcb` (X11) | Vulkan wgpu backend；需安装 xcb/xkb 库 |
| **Linux Wayland** | Ubuntu 22.04 LTS + Compositor | Stage 4 降级支持 | `wayland`（PIE 使用 texture-share 路径） | 有一帧延迟；Stage 6 升级为 EGLImageKHR |

> Editor 的 *game runtime* 平台支持（Windows Stage 2 完成；Linux/macOS Stage 6）与 *Editor 自身运行平台* 无关。Editor 可以在 macOS/Linux 上编辑并 PIE，只要 game runtime provider 也在该平台上有实现（AstraVN Native 使用纯 Rust，三平台均可编译）。

### 16.2 字体

| 平台 | 主字体 | CJK 来源 |
| --- | --- | --- |
| Windows | Noto Sans SC subset（捆绑）→ fallback 微软雅黑 | 捆绑 |
| macOS | Noto Sans SC subset（捆绑）→ fallback PingFang SC | 捆绑 |
| Linux | Noto Sans SC subset（捆绑）→ fallback WenQuanYi Micro Hei | 捆绑（Linux 尤其必要）|

Noto Sans SC 子集由 CI 中 `pyftsubset` 生成（约 5 MB），存放于 `Editor/Resources/fonts/NotoSansSC-subset.ttf`。各平台发行包一律含此文件。

### 16.3 菜单栏

- **macOS**：Qt 自动将 `QMenuBar` 放在屏幕顶部（macOS 全局菜单栏），不需要手动处理。
- **Windows / Linux**：菜单栏内嵌在主窗口内（Qt 默认行为）。
- 菜单栏 QML 统一用 `MenuBar` + `Menu` 声明，不直接操作 `QMenuBar`；cxx-qt bridge 负责把平台行为差异屏蔽在 C++ 侧。

### 16.4 系统文件选择器（AstraEditor.FilePicker）

`AstraEditor.FilePicker`（对插件暴露的接口）底层调用 `QFileDialog`，Qt 6.5 在各平台使用原生对话框：

| 平台 | 底层实现 |
| --- | --- |
| Windows | IFileOpenDialog (Vista Shell API) |
| macOS | NSOpenPanel |
| Linux | GTK 3/4 FileChooser（需要 `xdg-portal` 或 GTK 集成）|

Linux 若缺少 `xdg-desktop-portal` 会退回 Qt 内置文件选择器（Qt 6.5 默认行为）。

### 16.5 IME（输入法）

- **Windows**：TSF / IMM32（Qt 自动支持）
- **macOS**：NSInputMethod（Qt 自动支持）
- **Linux**：IBus / Fcitx5（需要 Qt `im-module` 正确配置；Editor 发行包 README 记录 `QT_IM_MODULE=fcitx5` 设置方式）

### 16.6 快捷键适配

Qt 在 macOS 上自动把 `Ctrl` 映射为 `Cmd`，大多数快捷键无需修改。以下键需要额外注意：

| 操作 | Windows / Linux | macOS |
| --- | --- | --- |
| Redo | `Ctrl+Y` / `Ctrl+Shift+Z` | `Cmd+Shift+Z` |
| 关闭面板 | `Ctrl+W` | `Cmd+W` |
| 系统 Undo | `Ctrl+Z` | `Cmd+Z` |

cxx-qt bridge 中统一用 `QKeySequence::StandardKey` 定义快捷键，由 Qt 负责平台映射，不硬编码平台特定按键码。

### 16.7 部署打包

| 平台 | 工具 | 输出 |
| --- | --- | --- |
| Windows | `windeployqt6 AstraEditor.exe` | 包含 Qt DLL 的目录，可选 NSIS installer |
| macOS | `macdeployqt6 AstraEditor.app` | `.app` bundle；Stage 6 加 notarization |
| Linux | `linuxdeploy` + AppImage plugin | `AstraEditor-x86_64.AppImage`；包含 Qt 库和字体 |

Editor 发行包（`kind = editor, packaged = false`）**不进入 `.astrapkg`**；由独立 Editor 发行流程输出。

### 16.8 wgpu backend 选择

| 平台 | 默认 wgpu backend | 备注 |
| --- | --- | --- |
| Windows | DirectX 12（Primary）→ Vulkan fallback | |
| macOS | Metal | Apple Silicon / Intel 均支持 |
| Linux X11/Wayland | Vulkan | 需要 Mesa 22.0+ 或 NVIDIA 470+；CI 用 SwiftShader |

PIE Viewport 使用与 game runtime 相同的 wgpu backend；Editor shell（Qt）不直接使用 wgpu，不受影响。

### 16.9 CI headless 测试（无显示器）

CI 所有平台使用 `QT_QPA_PLATFORM=offscreen`（Qt 内置 offscreen backend），跳过真实 GPU 渲染，仅验证 Bridge 逻辑和数据流。PIE Viewport GPU smoke 在带 GPU 的 self-hosted runner 上单独运行（Stage 4 可选，Stage 6 必须）。

---

## 17. 验收标准

| 面板 | 验收条件 |
| --- | --- |
| Project Wizard | 创建 NativeVN 项目 → 加载 → 显示所有面板空状态 |
| Script Editor | 编写 `.astra` → 语法高亮 → 错误 marker → compile → source map badge |
| Graph Editor | 从 `.astra` 生成 Graph → dagre 布局 → 节点编辑 → source roundtrip identity check |
| PIE Viewport | 启动 PIE → VN 运行 → 暂停/恢复 → 停止 |
| Inspector | 选中节点 → PropertySystem 渲染 → 值修改 → undo → redo |
| Debugger | PIE 中 → Actor/StateMachine 状态可见 → source span 可跳转 |
| Content Browser | VFS catalog 显示 → 资产缩略图 → 拖拽导入 |
| Plugin Manager | 插件 enable/disable → 受影响 surface 显示诊断 |
| Release Gate | 运行 gate → report 渲染 → blocked check 可跳转 source |
| AI Review Queue | AI 生成 > 5 行 → Review Queue 显示 5 步确认 → Apply → undo checkpoint |

```bash
cargo test -p astra-editor-bridge editor_creator_loop
cargo test -p astra-editor-bridge editor_target
cargo test -p astra-editor-bridge plugin_manager
cargo test -p astra-editor-bridge release_gate_panel
```

Expected report schema: `astra.editor_report.v1`
