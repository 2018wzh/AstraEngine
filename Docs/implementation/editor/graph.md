# AstraEditor Graph / Timeline / FilterGraph 设计

Graph Editor、Timeline Editor 和 FilterGraph/AudioGraph Editor 是 AstraVN 核心创作面。三者共享 NodeEditor-Qt 渲染框架，数据层分别由 `astra-vn-editor` 的 `VisualNode`/`TimelineTrack` 和 `astra-vn-presentation` 的 FilterGraph/AudioGraph 节点类型提供。

## 1. Graph Editor 架构

### 1.1 技术选型：NodeEditor-Qt

使用 NodeEditor-Qt（BSD-3-Clause，原名 nodeeditor/qtnodeeditor）作为渲染引擎。Bridge 侧通过 C++ 封装层（`GraphEditorAdapter.h/.cpp`）将 NodeEditor-Qt 与 cxx-qt 连接。

```
Rust astra-vn-editor::VisualNode (DTO)
  → Bridge runtime_provider.rs  → GraphNodeDto (JSON)
  → QML GraphEditor.qml         → C++ GraphEditorAdapter
  → NodeEditor-Qt               （渲染 + 用户交互）
  → 用户编辑事件
  → C++ GraphEditorAdapter      → QML signal (nodeMoved / nodeEdited / connectionChanged)
  → Bridge                      → source patch → astra-vn-script compile
  → CompileReport               → QML (conflict badge 或成功高亮)
```

`GraphEditorAdapter` 是 `QObject` 子类，通过 cxx-qt `#[qobject]` 或 `extern "C"` 与 Rust bridge 通信：

```cpp
// Editor/Source/App/astra-editor/src/GraphEditorAdapter.h
class GraphEditorAdapter : public QObject {
    Q_OBJECT
public:
    void loadGraph(const QJsonArray& nodes, const QJsonArray& connections);
    void loadLayout(const QJsonObject& layoutMeta);  // 作者元数据，不含 .astra 内容
    void applyConflictBadge(const QString& nodeId);
    void clearConflictBadges();

signals:
    void nodeParameterChanged(const QString& nodeId, const QString& paramName, const QJsonValue& value);
    void connectionChanged(const QString& fromNode, const QString& fromPort,
                           const QString& toNode,   const QString& toPort);
    void nodeMoved(const QString& nodeId, double x, double y);  // 只更新布局元数据
};
```

### 1.2 规模目标与性能策略

Stage 4 单个 Graph 视图性能目标：≤ 500 节点。

- NodeEditor-Qt 3.x 内置视口裁剪（只渲染可见节点），500 节点在常规 FHD 屏上无压力。
- 节点数超出 500 时，Editor 提示「当前 Graph 较大，建议按场景分割」，但不强制阻断。
- 超大项目（全局 Graph）通过「分场景/分章节 Graph」控制规模：每次只打开一个 scene 或 chapter 的 Graph。
- Stage 6 后如有需求，可在 Bridge 侧启用 LOD（缩放到一定比例时只显示节点摘要）。

### 1.3 初始布局：dagre 自动布局

首次从 `.astra` 生成 Graph 时（无已保存布局元数据），使用 dagre 算法（Rust crate `dagrs` 或 `petgraph` + 自研 layered layout）计算初始节点位置：

```rust
// bridge/src/lib.rs (graph layout helper)
pub fn compute_dagre_layout(nodes: &[VisualNode], edges: &[VisualEdge]) -> Vec<(StableId, f64, f64)> {
    // Sugiyama-style layered layout：
    // 1. 拓扑排序（entry → terminal）
    // 2. 按层分配 y 坐标
    // 3. 按列分配 x 坐标（barycenter heuristic 减少交叉）
    ...
}
```

布局元数据作为作者元数据（`visual_layout.json`）保存在项目目录下，**不写入 `.astra` 正文**。用户拖拽节点后，`nodeMoved` 信号触发布局元数据更新（不触发 source patch，不触发 compile）。

### 1.4 节点类型（AstraVN 内置命令集）

| 命令 | 节点颜色 | 端口 |
| --- | --- | --- |
| `say` / `narrate` | `#3d5a8a`（蓝灰） | in, out, character? |
| `choice` | `#6b5c1e`（金棕） | in, option_1_out, option_2_out, … |
| `jump` | `#4a4a4a`（灰白） | in, target_label |
| `wait` | `#4a3a6b`（紫） | in, out, duration |
| `set_variable` | `#2d5a2d`（绿） | in, out |
| `if_condition` | `#6b3a1e`（橙褐） | in, true_out, false_out |
| `call_luau` | `#6b2d6b`（品红） | in, out, args… |
| `scene_enter` | `#1e3a5a`（深蓝） | out |
| `scene_exit` | `#3a1e1e`（深红） | in |
| 插件扩展节点 | 插件 schema 中定义 | 插件 schema 中定义 |

节点颜色、图标和端口 schema 由 `astra-vn-commands` 的命令 schema 和插件 QML `AstraGraphNodeStyle` 提供；Graph Editor 从 `RuntimeEditorMetadata.graph_node_styles` 读取。

### 1.5 Source Roundtrip

编辑流程（参见 `editor-visual-protocol.md`）：

```
source → parse → CompiledStory IR → VisualNode 列表
  ↑                                       ↓
 compile (source patch)         用户编辑节点参数/连线
  ↑                                       ↓
source map identity check        Bridge.apply_graph_edit(patch)
  ↑                                       ↓
通过 → Graph 刷新               patch → astra-vn-script::compile()
  ↓
失败 → source map identity check fail
     → Graph 显示 conflict badge（橙色 ⚠ 图标）
     → Editor 显示「源文件已外部修改，请解决冲突」
     → 进入 Review Queue
```

**明确保存**策略：

- 用户**移动节点**：只更新布局元数据，不触发 source patch，不触发 compile。
- 用户**修改参数/连线**：触发 `Bridge.apply_graph_edit(patch)`，Bridge 调用 `astra-vn-script` 生成 source patch + compile → identity check。
- 外部 `.astra` 文件修改（检测到文件 mtime 变化）：Graph 显示 conflict badge，用户手动触发解决。

---

## 2. Timeline Editor

### 2.1 数据源

Timeline 数据来自 `astra-vn-editor` 的 `TimelineTrack`：

```rust
// astra-vn-editor (已实现)
pub struct TimelineTrack {
    pub track_id:   StableId,
    pub command_id: StableId,
    pub lane:       TimelineLane,
    pub clips:      Vec<TimelineClip>,
    pub fences:     Vec<FenceRef>,
}

pub struct TimelineClip {
    pub clip_id:    StableId,
    pub start_tick: u64,
    pub end_tick:   u64,
    pub asset_ref:  Option<VfsUri>,      // 音频/视频/图像资产
    pub parameters: serde_json::Value,  // 命令参数
}
```

### 2.2 QML Timeline 渲染

```qml
// panels/TimelineEditor.qml
Item {
    // 顶部工具栏：时间刻度、播放头、对齐选项
    TimelineRuler { id: ruler; model: bridge.timelineRulerModel }

    // 轨道区域：每个 TimelineTrack 一行
    ListView {
        model:    bridge.timelineTrackModel  // QAbstractListModel
        delegate: TimelineTrackRow {
            trackName:  model.trackName
            clips:      model.clips
            fences:     model.fences

            // 拖拽 clip
            onClipMoved: function(clipId, newStartTick) {
                bridge.moveTimelineClip(clipId, newStartTick)
            }

            // 调整 clip 长度
            onClipResized: function(clipId, newEndTick) {
                bridge.resizeTimelineClip(clipId, newEndTick)
            }
        }
    }

    // 底部：缺失资产警告、fence 泄露警告
    TimelineStatusBar { warningsJson: bridge.timelineWarningsJson }
}
```

### 2.3 Fence 显示

`FenceRef` 对应 `astra-runtime` 的 `Fence`（同步屏障）。Timeline 中 fence 渲染为跨轨道的竖线，hover 显示 fence id + 描述。fence 泄露（fence 开启但未关闭）显示为红色竖线 + 状态栏警告。

### 2.4 媒体预览

资产缩略图在 clip 上展示（图像为缩略图，音频为波形，视频为首帧）。缩略图读取 Content Browser 的持久化缓存（`.astra-cache/thumbnails/`）；未缓存时显示加载中动画。

Timeline Editor 暂不支持实时媒体播放预览（Stage 5 实现，需要 kira/wgpu 集成）。

---

## 3. FilterGraph / AudioGraph Editor

### 3.1 复用 Graph Editor 框架

FilterGraph 和 AudioGraph 作为特殊节点类型接入 NodeEditor-Qt，与 VN Graph 共用同一 `GraphEditorAdapter` 但使用不同的节点 schema。

```rust
// Bridge 侧：三种 Graph 模式
pub enum GraphEditorMode {
    VnStoryGraph,   // VisualNode 来自 astra-vn-editor
    FilterGraph,    // FilterGraphNode 来自 astra-vn-presentation::FilterGraph
    AudioGraph,     // AudioGraphNode 来自 astra-vn-presentation::AudioGraph
}
```

### 3.2 Stage 4 实现范围

Stage 4：
- FilterGraph/AudioGraph 节点可视化（节点框、端口、连线）
- 读取 `astra-vn-presentation` 的节点 schema 渲染节点类型和端口
- 连线编辑（添加/删除 connection）写回 Luau policy override

Stage 5 新增：
- 实时 preview（headless CPU filter 执行后渲染预览帧）
- 参数动画曲线编辑

### 3.3 典型 FilterGraph 节点类型

| 节点 | 说明 | 端口 |
| --- | --- | --- |
| `blur` | 高斯模糊 | image_in, image_out, radius |
| `color_grade` | 色调/饱和度/亮度调整 | image_in, image_out |
| `dissolve` | 淡入淡出 | image_a, image_b, alpha, image_out |
| `add_text` | 叠加文字层 | image_in, text, image_out |
| Luau policy 节点 | 插件 Luau policy 提供 | 由 policy schema 决定 |

---

## 4. 验收标准

```bash
# Graph/Timeline 编辑闭环测试（S4-EDITOR-04）
cargo test -p astra-editor-bridge graph_timeline_edit
```

| 测试 | 描述 |
| --- | --- |
| `graph_load` | 从 CompiledStory 生成 Graph，dagre 布局 |
| `graph_edit_roundtrip` | 修改节点参数 → source patch → compile → identity check pass |
| `graph_conflict_badge` | 外部修改 `.astra` → Graph 显示 conflict badge |
| `timeline_clip_move` | 拖动 clip → tick 更新 → policy override 写回 |
| `fence_leak_warning` | 构造 fence 泄露场景 → 状态栏警告出现 |
| `filtergraph_connection` | FilterGraph 节点连线 → policy override → compile |
