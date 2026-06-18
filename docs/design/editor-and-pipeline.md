# Editor 与构建流水线设计

## 1. Editor 目标

AstraEditor 是面向模块化 2D/VN 创作的工具链。它完整服务 Astra 原生脚本、Graph、Timeline、FilterGraph 和 VN 模块。
旧 VN 运行、增强和翻译由独立 AstraEmu Toolkit 承担，不进入 Editor 创作流程。

## 2. Editor 模块

```text
AstraEditor
├─ Project Browser
├─ Asset Browser / Asset Editor
├─ Scene / Actor Inspector
├─ Script Editor
├─ Graph Editor
├─ StateMachine Editor
├─ Timeline Editor
├─ Filter Graph Editor
├─ Runtime Debugger
├─ Editor Copilot MCP / Content Generation MCP / Review Queue
└─ Build / Package / Release Gate
```

Editor 依赖 Runtime public API；Runtime 不依赖 Editor。

## 3. 原生创作链路

```text
Text Source / Binary Assets
  -> Sidecar / Schema Validation
  -> AssetRegistry
  -> Script / Graph / Timeline Compile
  -> Preview / PIE
  -> Cook
  -> Package
```

PIE 使用同一 Actor、EventBus、StateMachineRuntime、ScriptRuntimeHost 和 Media 管线。编辑器调试层只提供输入、overlay 和 inspector。

## 3.1 Creator Workflow Contracts

Editor workflow 的每个入口都必须定义输入、输出、可撤销事务、preview 状态、保存行为和 diagnostics。
所有 mutation 先形成 structured patch；写入 canonical source 后标记 dirty，并由 undo/redo transaction 记录。

### Project Wizard / Template Browser

- 输入：template descriptor、项目名、目标目录、默认语言、目标平台、启用模块。
- 输出：`.astra.yaml`、`Content` 目录、默认脚本、默认场景、默认 filter profile、sample asset sidecar。
- Runtime API：不直接启动 runtime；只生成 canonical source 并调用 schema validation。
- Undo/Redo：项目创建作为单个外部事务，可在未打开前删除生成目录；打开后使用 source control 或 delete project。
- Preview/Save：Template Browser 只预览模板 metadata 和 sample 截图；完成向导才写入磁盘。
- 失败诊断：目录不可写、template 缺依赖、模块不可用、schema 无效。
- 验收：新项目可立即运行 `validate -> PIE -> cook -> package`。

### Content Browser / Asset Import Wizard

- 输入：外部文件、import preset、目标 Content 路径、license profile、tag/default metadata。
- 输出：source asset、`.asset.yaml` sidecar、AssetRegistry entry、可选 AI review item。
- Runtime API：通过 AssetRegistry、Importer、Cooker public service；不直接访问 renderer/audio handle。
- Undo/Redo：导入事务包含文件复制、sidecar、registry refresh；撤销必须移除新增 source 或转为 orphan draft。
- Preview/Save：导入前可预览 decode metadata；接受后写 Content，registry refresh 后进入 dirty state。
- 失败诊断：重复 AssetId、缺失 source、unsupported type、license 缺失、foreign mount 违规。
- 验收：导入角色立绘、背景、语音、字体和 filter profile 后可在 PIE 中引用。

### Script / Graph / Timeline / FilterGraph Editors

- 输入：canonical source、Property schema、Asset refs、Actor/Component refs。
- 输出：文本源、Graph/Timeline source、diagnostics、preview commands。
- Runtime API：通过 ScriptRuntimeHost、RuntimeEventBus、Presentation extraction 和 headless preview。
- Undo/Redo：node/track/text edit 均记录 source-level patch，不记录 runtime transient state。
- Preview/Save：preview 使用 headless runtime path；保存只写 canonical source 和 debug symbol/source map。
- 失败诊断：parse error、missing label、broken asset ref、invalid timeline target、filter target 无效。
- 验收：编辑器预览与 packaged runtime 使用相同 source 和 runtime path。

### Inspector / Details Panel

- 输入：PropertySystem descriptor、selected object snapshot、edit policy。
- 输出：structured patch、undo transaction、dirty state、validation diagnostics。
- Runtime API：通过 inspector DTO 和 command API；不持有 C++ object pointer。
- Undo/Redo：每个 property edit 是 typed patch；批量编辑可合并事务。
- Preview/Save：preview edit 可应用到 PIE runtime overlay；保存时只写 source object 或 review item。
- 失败诊断：read-only field、requires review、schema mismatch、migration required。
- 验收：Actor、Component、Asset、StateMachine、Script state 均可查看；可编辑字段可 undo/redo。

### PIE / Runtime Debugger

- 输入：project config、selected map/scene/script entry、runtime profile。
- 输出：running RuntimeWorld、event log、trace、save/replay snapshot、diagnostics。
- Runtime API：同 packaged runtime，额外启用 inspector/debugger hooks。
- Undo/Redo：PIE runtime change 默认 transient；选择 promote to source 时生成 reviewable patch。
- Preview/Save：save snapshot 与 replay 输出写 Saved；不污染 Content。
- 失败诊断：runtime init failure、asset load failure、script runtime failure、save mismatch。
- 验收：可暂停、单步、选择、保存、恢复、replay、定位 event/state mismatch。

### Cook / Package / Release Gate

- 输入：canonical source、module policy、release profile、target platform。
- 输出：Cooked assets、package manifest、release report、blocking diagnostics。
- Runtime API：CLI、Editor、MCP 共用同一 validation/cook/package service。
- Undo/Redo：Cooked、Package 和 DDC 不参与 undo；失败只产生 diagnostics 和 report。
- Preview/Save：Package panel 预览 manifest、size、module list、blocking diagnostics。
- 失败诊断：schema、asset dependency、plugin permission、AI review、runtime package eligibility。
- 验收：package 可在无 Editor 环境启动。

Editor layout preset 示例：

```yaml
id: astra.layout.vn_authoring
panels:
  - id: astra.content_browser
    dock: left
    visible: true
  - id: astra.script_graph
    dock: center
    visible: true
  - id: astra.inspector
    dock: right
    visible: true
  - id: astra.runtime_debugger
    dock: bottom
    visible: false
commands:
  astra.project.package: Ctrl+Shift+B
  astra.pie.play: F5
scope:
  project_default: true
  user_override: true
```

Graph/Timeline source schema 最小字段：

```yaml
id: native:/Graphs/Opening
schema: astra.graph.vn.v1
nodes:
  - id: node.say.001
    type: astra.vn.say
    source_location: Scripts/opening.astra:12
    properties:
      speaker: native:/Characters/Alice
      text_key: loc:/opening/alice_001
edges:
  - from: node.say.001.out
    to: node.choice.001.in
debug_symbols:
  source_map: Saved/DerivedDataCache/Graphs/Opening.sourcemap
hot_reload:
  require_state_compatibility: true
```

Asset Editor 可从 Editor Content Generation MCP 发起多模态生成、修改和增强。生成结果先进入 draft/review，不直接成为正式资产：

```text
Asset Editor Request
  -> Editor Content Generation MCP / Boundary Manager
  -> AI Provider
  -> Draft Asset + Sidecar Draft
  -> Preview / Variants
  -> Review Queue
  -> Import / Edit / Reject
  -> Content + Sidecar
  -> AssetRegistry
```

第一目标态覆盖文本、图像、音频、语音、视频和动画草稿。导入时必须生成稳定 AssetId、license、review 状态和审计链接；拒绝或取消的 draft 不进入 AssetRegistry、Cook 或 Package。

## 4. Cook / Package

Cook 阶段负责：

- YAML + JSON Schema 校验。
- Asset sidecar 扫描和 registry 生成。
- AI-generated asset sidecar、license、review 状态和审计链接校验。
- 脚本、Graph、Timeline、StateMachine 编译。
- 图片、音频、字体、FilterProfile 和 shader cook。
- 本地化表生成。
- AI 审计报告。
- AstraEmu Toolkit 不参与此流程；旧 VN 内容不作为 NativeVN source content 导入。

Package 默认生成 deterministic build，不包含运行时 AI Provider 和 authoring-only 模块。

## 5. Release Gate

发布前必须检查：

- 文本源和 sidecar 有效。
- 脚本/Graph/Timeline/StateMachine 可编译。
- AssetId、依赖、本地化 key 和 FilterProfile 完整。
- 未审核 AI 内容为 0，除非发布模式允许。
- AI-generated asset 均有 sidecar、license、Generation Audit 和 accepted review 状态。
- Runtime AI、MCP、Provider、Audit 模块权限与发布模式一致。
- 插件 ABI、权限、依赖闭包和 packaged eligibility 有效。

## 6. 测试

测试层级：

- Unit：Core、AssetId、PropertySystem、EventBus、StateMachineRuntime。
- Integration：Actor World、ScriptRuntimeHost、FilterGraph、Save/Replay。
- Headless：VN 路径、AI Intent。
- Editor Smoke：项目打开、资产导入、PIE、Inspector。
- Release Gate：schema、AI、plugin、package policy。

Creator acceptance：

- 新创作者能从模板创建 VN 项目，导入角色/背景/音频，写对白和选择，PIE 预览，调试事件并打包。
- 工具作者能添加 Editor panel，不修改 Runtime。
- 插件作者能替换 Renderer2D/TextLayout/Audio provider，并通过 Release Gate。
- AI Copilot 能提出脚本修复，AI 内容生成能导入 draft，二者都经过 Review Queue。
- 旧 VN 运行和现代化由 AstraEmu Toolkit 文档单独验收。
