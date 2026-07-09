# AstraEditor 前端设计文档

本目录包含 AstraEditor 前端（Qt/QML shell + Rust bridge）的完整设计稿和实现计划。设计对应 Stage 4 全部工作项（`REOPENED_SPEC`），不表示代码已存在。

## 阅读顺序

| 文档 | 内容 |
| --- | --- |
| [shell.md](shell.md) | cxx-qt bridge、Dock 布局、设计系统、面板生命周期、构建集成 |
| [graph.md](graph.md) | Graph Editor（NodeEditor-Qt）、Timeline Editor、FilterGraph/AudioGraph Editor、source round-trip |
| [script-editor.md](script-editor.md) | Script Editor（tree-sitter + ropey）、语法高亮、source map badge、查找/替换 |
| [ai-copilot.md](ai-copilot.md) | AI Copilot（inline hint + Review Queue）、MCP session、Trusted session、audit chain |

## 关联文档

- [ADR 0002](../../adr/0002-editor-ui-qt.md)：Editor 使用 Qt/QML + Rust core（高层决策）
- [ADR 0013](../../adr/0013-cxx-qt-bridge.md)：cxx-qt 作为具体 bridge 实现（本轮补充决策）
- [editor-workflow.md](../editor-workflow.md)：UE 级 creator workflow、面板状态、Bridge API 定义
- [editor-visual-protocol.md](../editor-visual-protocol.md)：Graph/Timeline 派生层、source round-trip contract
- [game-runtime-provider.md](../game-runtime-provider.md)：`RuntimeEditorMetadata` 结构和 Provider 切换协议
- [provider-plugin-api.md](../provider-plugin-api.md)：插件 descriptor、extension point、Editor phase 生命周期
- [ai-mcp-runtime.md](../ai-mcp-runtime.md)：AI/MCP runtime、Copilot、audit、provider-free replay

## 设计决策摘要（2026-07 grill-me 确认）

| 决策维度 | 结论 |
| --- | --- |
| Rust↔Qt Bridge | **cxx-qt**（KDAB），`#[qml_element]`/`#[qproperty]`/`#[qsignal]`/`#[qinvokable]` |
| Qt 版本 | **Qt 6.5 LTS**；Stage 6 评估升级 6.8+ |
| QML 数据绑定 | 混合：列表 → `QAbstractListModel`；单对象 → `Q_PROPERTY`；复杂报告 → JSON DTO |
| PIE Viewport | `AstraPlatform::create_surface` 统一收束：由平台适配层处理窗口关联与 Wayland 自动降级，Editor 仅持有不透明 `SurfaceToken` |
| Graph 规模目标 | 单 Graph 视图 ≤ 500 节点，超大项目按场景/章节分 Graph |
| Graph 初始布局 | **dagre** 自动布局；布局元数据存项目元数据，不写入 `.astra` |
| Dock 布局 | 3 个预设（Default / Scripting / VN Graph），支持持久化和 Tab 合并 |
| 插件 QML 沙箱 | 禁止 `QtQuick.Dialogs`；文件操作走 `AstraEditor.FilePicker` 接口 |
| Script Editor 功能 | Stage 4：行号 gutter、错误 marker、source map badge、查找/替换 |
| 文本引擎 | **tree-sitter**（`.astra` grammar）+ **ropey**（rope buffer），为 astra-lsp 铺路 |
| Undo 模型 | 分层栈：Script Editor 内 ropey 局部 undo → compile → 全局 patch 历史 |
| 缩略图缓存 | 混合：已 cook 资产持久化到 `.astra-cache/thumbnails/`，脏资产每次重生成 |
| AI inline hint | ≤ 5 行直接写入 + audit event；> 5 行走 Review Queue 五步确认 |
| 快捷键 | Stage 4 固定；Stage 5 实现可定制 |
| FilterGraph/AudioGraph | 复用 Graph Editor 框架，Stage 4 基础可视化，Stage 5 加 preview |
| Save/Replay Inspector | Stage 4：section 结构浏览 + PIE 内 seek |
| 本地化 | Stage 4 中英双语 UI（Qt Linguist），**捆绑 Noto Sans SC 子集**（~5 MB），三平台一致 |
| 构建集成 | Editor crate 独立 CI job（需 Qt 6.5 LTS）；**Windows / macOS / Linux 三平台 matrix**；默认 `cargo test --workspace` 不含 Editor |
| 桌面平台 | **Windows**（Stage 4 一级）/ **macOS 13+**（Stage 4 同步）/ **Linux X11**（Stage 4 同步）/ **Linux Wayland**（Stage 4 降级 texture-share） |
| Stage 4 实现优先级 | 创作流程优先：Script Editor → Graph → PIE 运行 VN 为第一里程碑 |

## Stage 4 工作项映射

| 工作项 ID | 目标 | 对应设计文档 |
| --- | --- | --- |
| `S4-EDITOR-01` | Qt/QML shell + Project Wizard + Bridge 骨架 | [shell.md](shell.md) |
| `S4-EDITOR-02` | PIE runtime bridge | [shell.md §PIE](shell.md) |
| `S4-PLUGIN-01` | Plugin Manager UI | [shell.md §Plugin Manager](shell.md) |
| `S4-EDITOR-03` | Inspector + Debugger | [shell.md §Inspector](shell.md) |
| `S4-EDITOR-04` | Graph/Timeline 编辑闭环 | [graph.md](graph.md) |
| `S4-EDITOR-05` | Package/Release Gate panel | [shell.md §Release Gate](shell.md) |
| `S4-EDITOR-RUNTIME-PROVIDER-01` | Runtime provider 切换 | [shell.md §Provider 切换](shell.md) |
| `S4-EDITOR-TARGET-01` | Editor target | [shell.md §Target](shell.md) |
| `S4-AI-01` ~ `S4-GATE-01` | AI/MCP 全部 | [ai-copilot.md](ai-copilot.md) |

## 验证

```bash
python Tools/check_docs.py
# Editor 独立 CI（需要 Qt 6.5 LTS）：
cargo test -p astra-editor-bridge editor_creator_loop
cargo test -p astra-editor-bridge plugin_manager
cargo test -p astra-editor-bridge release_gate_panel
cargo test -p astra-ai runtime_ai_replay
cargo test -p astra-mcp capability_protocol
```
