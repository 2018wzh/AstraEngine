# Editor UI 与 AI 协作原型

状态：Target UI Concept

## 1. 目标

编辑器 UI 面向可定制 2D/VN 创作，不做营销式界面。第一屏是实际工作台：项目树、场景/Actor 视图、脚本/Graph/Timeline、Inspector、Runtime Debugger 和 AI Review Queue。

## 2. 主布局

```text
Top Bar
  Project / Build / Run / Package / Release Gate

Left Dock
  Asset Browser / Asset Editor / Scene Tree / Actor Tree

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
- 输出 inline suggestion、patch proposal、asset draft、localization draft 或 runtime intent preview。
- 支持 chat-driven patch、batch refactor、diagnostics explanation 和 asset generation request。
- 所有正式内容变更进入 Review Queue，除非是显式 trusted MCP direct write。
- 每次生成写 Generation Audit；每次工具副作用写 Operation Log。

## 4. Asset Editor AI 生成

Asset Editor 中的 AI 生成入口面向实际资产编辑，而不是独立聊天：

- 参数面板：asset type、prompt、reference assets、target folder、license/profile、output constraints。
- 生成预览：文本、图像、音频、语音、视频或动画 draft，支持 variants 对比。
- Sidecar draft：显示稳定 ID 候选、tags、origin、license、review 状态和 audit 链接。
- Review Queue：导入、编辑、拒绝、重新生成；默认不直接写正式 Content。
- 审计状态：显示 provider、session、prompt/context hash、output hash 和 trusted write operation。

## 5. Runtime Debugger

Runtime Debugger 必须能查看：

- Actor tree、Component data、StateMachine 当前状态。
- EventBus 最近事件、queued events、ControlPolicy lock。
- ScriptRuntimeHost 当前 runtime、entry、call stack 或 VM 状态。
- FilterGraph active profile。
- AI committed intent。
- Legacy compat VM state。

## 6. Compatibility Inspector

旧 VN 项目显示：

- 外部项目 probe 结果。
- VFS/package mount 状态。
- legacy asset refs。
- VM/timeline/opcode 状态。
- 未支持 API 统计。
- 现代化配置和 FilterProfile。

## 7. Qt 实现映射

第一阶段可用 Qt 实现 dockable editor shell。Scene View 通过引擎渲染输出嵌入；脚本、Graph、Timeline、FilterGraph、Inspector 均调用 Runtime public DTO，不访问内部 native object。
