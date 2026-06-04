# Editor UI 与 AI 协作原型

状态：Target UI Concept

## 1. 目标

编辑器 UI 面向可定制 2D/VN 创作，不做营销式界面。第一屏是实际工作台：项目树、场景/Actor 视图、脚本/Graph/Timeline、Inspector、Runtime Debugger 和 AI Review Queue。

## 2. 主布局

```text
Top Bar
  Project / Build / Run / Package / Release Gate

Left Dock
  Asset Browser / Scene Tree / Actor Tree

Center
  Scene View / Script Editor / Graph Editor / Timeline / FilterGraph

Right Dock
  Inspector / Component Properties / StateMachine Debug

Bottom Dock
  Output Log / Event Log / Runtime Debugger / Review Queue / Compatibility Diagnostics
```

## 3. AI Workbench

AI Workbench 不直接覆盖正式内容：

- 读取上下文：角色、设定、当前脚本、Graph、Timeline、资产 metadata。
- 输出 patch、asset draft、localization draft 或 runtime intent preview。
- 所有正式内容变更进入 Review Queue，除非是显式 trusted MCP direct write。
- 每次生成写 Generation Audit；每次工具副作用写 Operation Log。

## 4. Runtime Debugger

Runtime Debugger 必须能查看：

- Actor tree、Component data、StateMachine 当前状态。
- EventBus 最近事件、queued events、ControlPolicy lock。
- ScriptRuntimeHost 当前 runtime、entry、call stack 或 VM 状态。
- FilterGraph active profile。
- AI committed intent。
- Legacy compat VM state。

## 5. Compatibility Inspector

旧 VN 项目显示：

- 外部项目 probe 结果。
- VFS/package mount 状态。
- legacy asset refs。
- VM/timeline/opcode 状态。
- 未支持 API 统计。
- 现代化配置和 FilterProfile。

## 6. Qt 实现映射

第一阶段可用 Qt 实现 dockable editor shell。Scene View 通过引擎渲染输出嵌入；脚本、Graph、Timeline、FilterGraph、Inspector 均调用 Runtime public DTO，不访问内部 native object。
