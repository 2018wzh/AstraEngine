# Editor UI AI Collaboration Prototype

状态：Draft  
原型路径：`Prototypes/AstraEditorUI`  
技术栈：Vue 3 + Vite + TypeScript

## 1. 目标

本原型用于验证 AstraEditor 的第一阶段信息架构和 AI 协作工作流。它不是 Qt 编辑器实现，也不连接真实 Runtime、MCP server 或 AI Provider。

设计重点：

- 以游戏编辑器式 Scene Viewport 为中心，而不是纯 IDE 布局。
- 保留 Content Browser、Inspector、Script Editor、PIE、Review Queue 和 Release Gate 的完整工作面。
- 把 AI 协作显式放在创作流程内：Context Builder、Boundary Manager、Diff/Patch、Review Queue、Audit。
- 区分普通 AI 建议和 MCP trusted direct write。前者默认进入 Review Queue；后者是显式受信开发工具能力，必须记录 Operation Log。

## 2. 信息架构

主窗口采用四区布局：

```text
App Shell
├── Activity Bar
├── Content Browser
├── Scene Viewport + Script / Story Panel
├── Inspector + Agent Workbench
└── Bottom Dock: Review Queue / RuntimeCommand Log / Release Gate
```

### 2.1 App Shell

职责：

- 显示项目名、项目路径、当前模式、dirty source 数量。
- 提供 `Authoring`、`PIE`、`Review`、`Release` 模式切换。
- 显示 AI policy、Editor MCP trusted session、Release Gate 状态。
- 提供 Light/Dark 主题切换。

Qt 映射：`QMainWindow` menu bar、tool bar、status widgets。

### 2.2 Content Browser

职责：

- 展示 Text-First 源文件、资产 sidecar、角色、设定和脚本。
- 支持按 scene、script、character、lore、audio 过滤。
- 选择脚本行或场景后更新中央视口、Inspector 和 Agent 上下文。

Qt 映射：`QTreeView` + `QListView`，数据源来自 AssetRegistry、project manifest 和文本源索引。

### 2.3 Scene Viewport

职责：

- 第一优先级展示当前 VN 场景预览：背景、立绘、对白框、音频和安全框。
- PIE 按钮通过同一 Runtime Services 启动预览，不复制运行逻辑。
- Release Gate 摘要以最小状态条提示当前可发布性。

Qt 映射：Scene Preview dock 或 central widget。后续应接入 Renderer2D 的预览输出或专用 viewport adapter。

### 2.4 Inspector + Agent Workbench

职责：

- Inspector 显示当前 scene、line、asset ref、expression、BGM 等可编辑属性。
- Agent Workbench 展示 Context Builder 输出、Boundary Decision、约束、redaction 和最新 patch。
- `Generate review patch` 只能生成建议并进入 Review Queue，不能直接覆盖 Canonical Project。

Qt 映射：Inspector 可由 VN Property System 驱动；Agent Workbench 是 Editor panel provider 的内置面板。

### 2.5 Bottom Dock

职责：

- Review Queue：显示 patch、before/after diff、stale 状态和 Accept/Edit/Reject/Defer 操作。
- RuntimeCommand Log：显示 PIE、AstraRuntime 和 Agent Preview 产生的命令。
- Release Gate：显示未审核 AI 内容、schema、资产依赖和 Runtime MCP / Generation policy 检查。
- Audit 摘要：显示 agent、model、target、作者动作、hash 关联。

Qt 映射：`QTabWidget` + table/list/detail widgets。Review 和 Audit 存储格式应与 `ai-collaboration.md` 中的 patch/audit schema 对齐。

## 3. UI 状态与类型

Vue 原型定义以下 mock 类型，后续 Qt/C++ 实现可映射为 DTO 或 model/view 数据源：

- `EditorWorkspaceState`：项目、模式、PIE、dirty files、AI/MCP 状态。
- `SceneSelection`：当前 scene、脚本路径、line id、speaker、stage asset refs。
- `AssetSummary`：资产 ID、类型、路径、标签、来源、健康状态。
- `AgentRunRequest`：任务类型、目标、provider、模型、温度、是否需要审核。
- `ContextSummary`：目标、`context_hash`、角色、设定、资产、约束、redaction。
- `BoundaryDecision`：是否允许、是否需要审核、可写目标、被阻止目标、原因。
- `ReviewPatch`：patch 类型、target、before/after、reason、status、agent、stale。
- `AuditEventSummary`：agent、model、target、作者动作、context/output hash。
- `RuntimeCommandLogEntry`：frame、command、payload、source。
- `ReleaseGateCheck`：检查项、说明、pass/warn/block 状态。

## 4. AI 协作边界

普通 AI 工作流：

```text
Selection
  -> Context Builder
  -> Boundary Manager
  -> Agent Execution
  -> Patch
  -> Review Queue
  -> Accept / Edit / Reject / Defer
  -> Audit Event
```

约束：

- AI 输出默认是 `AISuggested`。
- AI 不能直接修改 Canon Lore、Cooked Content、DerivedDataCache 或 packaged runtime。
- 进入 Canonical Project 前必须有人类动作。
- Release Gate 必须能阻塞未审核 AI 内容。

Editor MCP trusted direct write 工作流：

```text
Trusted MCP Tool Call
  -> Workspace Boundary Check
  -> Text Source Write
  -> Operation Log
  -> Optional Validation / Audit
```

差异：

- Editor MCP trusted session 是用户显式开启的 Developer 工具能力。
- Editor MCP 直接写入不强制进入 Review Queue，但 mutating tool 必须记录 Operation Log。
- 如果 MCP 写入内容标记为 AI-generated 或 AI-edited，应同时写入 Generation Audit Log。
- MCP 仍不得暴露明文密钥、未授权外部路径或 ECS/EnTT 内部状态。

## 5. Vue 原型组件拆分

```text
src/App.vue
src/components/AppShell.vue
src/components/ActivityBar.vue
src/components/ContentBrowser.vue
src/components/SceneViewport.vue
src/components/ScriptPanel.vue
src/components/InspectorAgent.vue
src/components/BottomDock.vue
src/components/StatusBadge.vue
src/data/seed.ts
src/types.ts
src/styles.css
```

实现原则：

- `App.vue` 只负责组合状态和模拟工作流。
- `seed.ts` 承载本地 mock 数据，不做真实 IO。
- `types.ts` 是 UI 契约的 TypeScript 表达。
- CSS variables 管理 Light/Dark 主题，避免把主题逻辑写散到组件中。
- 所有按钮改变本地 UI 状态，避免假控件。

## 6. 验收标准

必须通过：

- `npm install`
- `npm run build`
- 浏览器打开 Vite dev server 后能看到完整编辑器 UI。
- 选择脚本行会更新 Scene Viewport、Inspector 和 Context Builder。
- 运行 Agent 会生成新的 Review Patch 并增加 Audit/RuntimeCommand 记录。
- Accept/Edit/Reject/Defer 会改变 patch 状态并写入 Audit 摘要。
- PIE 会切换播放状态并写入 RuntimeCommand Log。
- 未审核 AI 内容会使 Release Gate 显示 block；清空 pending 后解除 block。

非目标：

- 不接入真实 AI Provider。
- 不启动真实 MCP server。
- 不调用真实 Runtime Services。
- 不作为最终 Qt UI 代码。
