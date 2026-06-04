# Editor 与构建流水线设计

## 1. Editor 目标

AstraEditor 是面向模块化 2D/VN 创作的工具链。它完整服务 Astra 原生脚本、Graph、Timeline、FilterGraph 和 VN 模块；对旧 VN 兼容项目提供只读脚本/反汇编/资源查看、运行时调试和现代化配置。

## 2. Editor 模块

```text
AstraEditor
├─ Project Browser
├─ Asset Browser
├─ Scene / Actor Inspector
├─ Script Editor
├─ Graph Editor
├─ StateMachine Editor
├─ Timeline Editor
├─ Filter Graph Editor
├─ Runtime Debugger
├─ AI Workbench / Review Queue
├─ Compatibility Inspector
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

## 4. 旧 VN 现代化链路

```text
External Game Directory
  -> Read-only VFS Mount
  -> Legacy Package Reader
  -> Compat Runtime / VM
  -> Modernization Profile
  -> Runtime Debugger / Inspector
```

Compatibility Inspector 支持查看 VM 状态、变量、缺失资源、未支持 API、FilterProfile、字体替换、UI 覆盖和高清替换。

## 5. Cook / Package

Cook 阶段负责：

- YAML + JSON Schema 校验。
- Asset sidecar 扫描和 registry 生成。
- 脚本、Graph、Timeline、StateMachine 编译。
- 图片、音频、字体、FilterProfile 和 shader cook。
- 本地化表生成。
- AI 审计报告。
- Legacy compatibility metadata 和 modernization profile 校验。

Package 默认生成 deterministic build，不包含运行时 AI Provider 和 authoring-only 模块。

## 6. Release Gate

发布前必须检查：

- 文本源和 sidecar 有效。
- 脚本/Graph/Timeline/StateMachine 可编译。
- AssetId、依赖、本地化 key 和 FilterProfile 完整。
- 未审核 AI 内容为 0，除非发布模式允许。
- Runtime AI、MCP、Provider、Audit 模块权限与发布模式一致。
- Legacy mount-only 项目没有复制外部原始资产。
- 插件 ABI、权限、依赖闭包和 packaged eligibility 有效。

## 7. 测试

测试层级：

- Unit：Core、AssetId、PropertySystem、EventBus、StateMachineRuntime。
- Integration：Actor World、ScriptRuntimeHost、FilterGraph、Save/Replay。
- Headless：VN 路径、AI Intent、legacy VM playback。
- Editor Smoke：项目打开、资产导入、PIE、Inspector。
- Release Gate：schema、AI、plugin、compat、package policy。
